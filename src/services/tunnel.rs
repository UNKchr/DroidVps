use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use flate2::read::GzDecoder;
use tar::Archive;

use crate::android::network;
use crate::container::chroot::ChrootRunner;
use crate::container::mount::MountManager;
use crate::error::DroidError;

const TAILSCALE_BIN_REL: &str = "usr/sbin/tailscaled";
const TAILSCALE_CLI_REL: &str = "usr/bin/tailscale";
const TAILSCALE_CLI_IN_CHROOT: &str = "/usr/bin/tailscale";
const TAILSCALE_PID_REL: &str = "run/tailscaled.pid";
const TAILSCALE_SOCKET_ARG: &str = "--socket=/run/tailscale/tailscaled.sock";
const TAILSCALE_STATE_REL: &str = "var/lib/tailscale/tailscaled.state";
const TAILSCALE_ENABLED_REL: &str = "etc/droidvps/tailscale.enabled";
const TAILSCALE_STABLE_JSON_URL: &str = "https://pkgs.tailscale.com/stable/?mode=json";
const TAILSCALE_FALLBACK_TGZ: &str = "tailscale_1.104.1_arm64.tgz";

const CLOUDFLARED_BIN_REL: &str = "usr/local/bin/cloudflared";
const CLOUDFLARED_PID_REL: &str = "run/cloudflared.pid";
const CLOUDFLARED_TOKEN_REL: &str = "etc/droidvps/cloudflared.token";
const CLOUDFLARED_ARM64_URL: &str =
    "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-arm64";

/// Proveedores de túnel soportados dentro del contenedor
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunnelProvider {
    Tailscale,
    Cloudflared,
}

impl TunnelProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tailscale => "Tailscale (userspace-networking)",
            Self::Cloudflared => "Cloudflare Tunnel (cloudflared)",
        }
    }
}

/// Estado operativo del subsistema de túneles
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TunnelStatus {
    Running {
        provider: TunnelProvider,
        pid: u32,
    },
    Stopped {
        configured: Option<TunnelProvider>,
    },
    NotConfigured,
}

/// Orquestador inteligente de túneles seguros (Tailscale / Cloudflare Tunnel) en el contenedor Alpine
pub struct TunnelService {
    rootfs: PathBuf,
}

impl TunnelService {
    /// Crea una nueva instancia del gestor de túneles para el `rootfs` especificado
    pub fn new<P: AsRef<Path>>(rootfs: P) -> Self {
        Self {
            rootfs: rootfs.as_ref().to_path_buf(),
        }
    }

    /// Inicia automáticamente el túnel si existe una configuración activa en `/etc/droidvps/`.
    /// Si el binario requerido (`tailscaled` o `cloudflared`) no está instalado, lo instala automáticamente.
    pub fn start_if_configured(&self) -> Result<TunnelStatus, DroidError> {
        if let Some((provider, pid)) = self.active_daemon() {
            return Ok(TunnelStatus::Running { provider, pid });
        }

        self.ensure_config_dir()?;

        // Prioridad 1: Token de Cloudflare Tunnel configurado en /etc/droidvps/cloudflared.token
        if self.has_cloudflared_token() {
            let pid = self.start_cloudflared()?;
            return Ok(TunnelStatus::Running {
                provider: TunnelProvider::Cloudflared,
                pid,
            });
        }

        // Prioridad 2: Tailscale habilitado o previamente autenticado
        if self.is_tailscale_configured() {
            let pid = self.start_tailscale()?;
            return Ok(TunnelStatus::Running {
                provider: TunnelProvider::Tailscale,
                pid,
            });
        }

        Ok(TunnelStatus::NotConfigured)
    }

    /// Flujo asistido para Tailscale: instala o actualiza los binarios estáticos oficiales,
    /// inicia `tailscaled` y ejecuta `tailscale up`.
    pub fn setup_and_authenticate_tailscale(
        &self,
        auth_key: Option<&str>,
    ) -> Result<u32, DroidError> {
        let pid = self.start_tailscale()?;
        let runner = ChrootRunner::new(&self.rootfs);

        println!("[*] Linking Tailscale node (follow the authentication link if it appears)...");
        match auth_key {
            Some(key) if !key.trim().is_empty() => {
                let auth_arg = format!("--authkey={}", key.trim());
                runner.run_command(
                    TAILSCALE_CLI_IN_CHROOT,
                    &[TAILSCALE_SOCKET_ARG, "up", &auth_arg],
                )?;
            }
            _ => {
                runner.run_command(TAILSCALE_CLI_IN_CHROOT, &[TAILSCALE_SOCKET_ARG, "up"])?;
            }
        }

        Ok(pid)
    }

    /// Flujo asistido para Cloudflare Tunnel: valida y guarda el token con permisos `0600`,
    /// descarga automáticamente el binario oficial `cloudflared` (`aarch64`) si no existe,
    /// e inicia el demonio en segundo plano.
    pub fn setup_and_start_cloudflared(&self, token: &str) -> Result<u32, DroidError> {
        let clean_token = token.trim();
        Self::validate_cloudflared_token(clean_token)?;

        self.ensure_config_dir()?;
        let token_path = self.rootfs.join(CLOUDFLARED_TOKEN_REL);
        fs::write(&token_path, format!("{}\n", clean_token))?;
        fs::set_permissions(&token_path, fs::Permissions::from_mode(0o600))?;

        self.start_cloudflared()
    }

    /// Instala o actualiza automáticamente Tailscale a la versión estática oficial más reciente
    /// e inicia `tailscaled` en modo userspace networking
    pub fn start_tailscale(&self) -> Result<u32, DroidError> {
        // Si detectamos que aún está instalado el paquete antiguo de Alpine apk, detenemos y migramos al binario oficial
        if self.is_legacy_apk_tailscale() {
            println!("[*] Outdated Alpine apk Tailscale package detected. Upgrading to latest official static release...");
            if let Some(old_pid) = self.tailscale_pid() {
                self.kill_process(old_pid);
                thread::sleep(Duration::from_millis(300));
            }
            self.prepare_runtime_environment()?;
            let runner = ChrootRunner::new(&self.rootfs);
            let _ = runner.run_command("/sbin/apk", &["del", "tailscale"]);
            self.install_official_tailscale()?;
        } else if let Some(pid) = self.tailscale_pid() {
            return Ok(pid);
        }

        self.prepare_runtime_environment()?;

        if !self.is_tailscale_installed() {
            self.install_official_tailscale()?;
        }

        let run_ts_dir = self.rootfs.join("run/tailscale");
        let state_ts_dir = self.rootfs.join("var/lib/tailscale");
        for dir in [&run_ts_dir, &state_ts_dir] {
            if !dir.exists() {
                fs::create_dir_all(dir)?;
            }
        }

        self.ensure_config_dir()?;
        let enabled_marker = self.rootfs.join(TAILSCALE_ENABLED_REL);
        if !enabled_marker.exists() {
            fs::write(&enabled_marker, b"enabled\n")?;
        }

        let runner = ChrootRunner::new(&self.rootfs);
        runner.spawn_daemon(
            "/usr/sbin/tailscaled",
            &[
                "--tun=userspace-networking",
                "--statedir=/var/lib/tailscale",
                TAILSCALE_SOCKET_ARG,
            ],
        )?;

        for _ in 0..40 {
            if let Some(pid) = self.tailscale_pid() {
                return Ok(pid);
            }
            thread::sleep(Duration::from_millis(100));
        }

        Err(DroidError::Rootfs(
            "tailscaled ran but no active process was detected in /proc".into(),
        ))
    }

    /// Descarga e instala los binarios estáticos oficiales de Tailscale (`arm64`) desde `pkgs.tailscale.com`
    fn install_official_tailscale(&self) -> Result<(), DroidError> {
        // Garantiza certificados CA en el contenedor para conexiones TLS de Tailscale
        let cert_marker = self.rootfs.join("etc/ssl/certs/ca-certificates.crt");
        if cert_marker.symlink_metadata().is_err() {
            let runner = ChrootRunner::new(&self.rootfs);
            runner.run_command("/sbin/apk", &["update"])?;
            runner.run_command("/sbin/apk", &["add", "--no-cache", "ca-certificates"])?;
        }

        network::ensure_host_dns_resolver();
        let tgz_filename = Self::resolve_latest_tailscale_tgz();
        let download_url = format!("https://pkgs.tailscale.com/stable/{}", tgz_filename);

        println!(
            "[*] Downloading official Tailscale static binaries ({}) ...",
            tgz_filename
        );

        let response = ureq::get(&download_url).call().map_err(|err| {
            DroidError::Rootfs(format!(
                "Failed to download Tailscale archive from {}: {}",
                download_url, err
            ))
        })?;

        let gz = GzDecoder::new(response.into_reader());
        let mut archive = Archive::new(gz);

        let sbin_dir = self.rootfs.join("usr/sbin");
        let bin_dir = self.rootfs.join("usr/bin");
        fs::create_dir_all(&sbin_dir)?;
        fs::create_dir_all(&bin_dir)?;

        let mut found_daemon = false;
        let mut found_cli = false;

        for entry_res in archive.entries()? {
            let mut entry = entry_res?;
            let path = entry.path()?;
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };

            if name == "tailscaled" {
                let dest = self.rootfs.join(TAILSCALE_BIN_REL);
                entry.unpack(&dest)?;
                fs::set_permissions(&dest, fs::Permissions::from_mode(0o755))?;
                found_daemon = true;
            } else if name == "tailscale" {
                let dest = self.rootfs.join(TAILSCALE_CLI_REL);
                entry.unpack(&dest)?;
                fs::set_permissions(&dest, fs::Permissions::from_mode(0o755))?;
                found_cli = true;
            }
        }

        if !found_daemon || !found_cli {
            return Err(DroidError::Rootfs(
                "Official Tailscale archive did not contain expected binaries".into(),
            ));
        }

        println!("\x1b[1;32m[✓] Official Tailscale static binaries installed successfully.\x1b[0m");
        Ok(())
    }

    /// Consulta el endpoint JSON oficial de Tailscale para obtener el nombre del tarball estable `arm64` más reciente
    fn resolve_latest_tailscale_tgz() -> String {
        if let Ok(resp) = ureq::get(TAILSCALE_STABLE_JSON_URL).call() {
            if let Ok(body) = resp.into_string() {
                if let Some(idx) = body.find("\"arm64\":") {
                    let rest = &body[idx + 8..];
                    if let Some(first_quote) = rest.find('"') {
                        let after_quote = &rest[first_quote + 1..];
                        if let Some(end_quote) = after_quote.find('"') {
                            let filename = &after_quote[..end_quote];
                            if filename.starts_with("tailscale_") && filename.ends_with(".tgz") {
                                return filename.to_string();
                            }
                        }
                    }
                }
            }
        }
        TAILSCALE_FALLBACK_TGZ.to_string()
    }

    /// Detecta si `tailscale` fue instalado mediante el paquete antiguo de Alpine `apk`
    fn is_legacy_apk_tailscale(&self) -> bool {
        let world_file = self.rootfs.join("etc/apk/world");
        if let Ok(content) = fs::read_to_string(world_file) {
            return content.lines().any(|line| line.trim() == "tailscale");
        }
        false
    }

    /// Descarga automáticamente `cloudflared` (si falta) e inicia el túnel con el token configurado
    pub fn start_cloudflared(&self) -> Result<u32, DroidError> {
        if let Some(pid) = self.cloudflared_pid() {
            return Ok(pid);
        }

        let token_path = self.rootfs.join(CLOUDFLARED_TOKEN_REL);
        if !token_path.exists() {
            return Err(DroidError::Rootfs(
                "No token is configured in /etc/droidvps/cloudflared.token".into(),
            ));
        }

        let token = fs::read_to_string(&token_path)
            .map_err(DroidError::Io)?
            .trim()
            .to_string();

        Self::validate_cloudflared_token(&token)?;

        self.prepare_runtime_environment()?;

        // Auto-instalación inteligente de dependencias y del binario si no existe en el contenedor
        self.ensure_cloudflared_installed()?;
        self.ensure_config_dir()?;

        let runner = ChrootRunner::new(&self.rootfs);
        runner.spawn_daemon(
            "/usr/local/bin/cloudflared",
            &[
                "tunnel",
                "--no-autoupdate",
                "--protocol",
                "http2",
                "--pidfile",
                "/run/cloudflared.pid",
                "--logfile",
                "/var/log/cloudflared.log",
                "run",
                "--token",
                &token,
            ],
        )?;

        for _ in 0..40 {
            if let Some(pid) = self.cloudflared_pid() {
                return Ok(pid);
            }
            thread::sleep(Duration::from_millis(100));
        }

        Err(DroidError::Rootfs(
            "cloudflared ran but did not register an active PID in /proc".into(),
        ))
    }

    /// Garantiza que el binario `/usr/local/bin/cloudflared` (`aarch64`) y su capa de
    /// compatibilidad (`gcompat` + `ca-certificates`) estén instalados.
    pub fn ensure_cloudflared_installed(&self) -> Result<(), DroidError> {
        // Verifica que Alpine tenga certificados raíz y compatibilidad glibc para el binario oficial arm64
        let ld_glibc = self.rootfs.join("lib/ld-linux-aarch64.so.1");
        if ld_glibc.symlink_metadata().is_err() {
            println!("[*] Installing glibc compatibility layer and CA certificates via apk...");
            let runner = ChrootRunner::new(&self.rootfs);
            runner.run_command("/sbin/apk", &["update"])?;
            runner.run_command(
                "/sbin/apk",
                &["add", "--no-cache", "ca-certificates", "gcompat"],
            )?;
        }

        if self.is_cloudflared_installed() {
            return Ok(());
        }

        // Asegura que el puente DNS UDP local (127.0.0.1:53) esté activo en el host antes de usar ureq
        network::ensure_host_dns_resolver();

        println!("[*] cloudflared binary not detected. Downloading the official version (linux-arm64)...");

        let bin_path = self.rootfs.join(CLOUDFLARED_BIN_REL);
        if let Some(parent) = bin_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        let temp_path = self.rootfs.join("usr/local/bin/cloudflared.tmp");

        let response = ureq::get(CLOUDFLARED_ARM64_URL).call().map_err(|err| {
            DroidError::Rootfs(format!(
                "Failed to download cloudflared from GitHub Releases: {}",
                err
            ))
        })?;

        let mut reader = response.into_reader();
        let mut temp_file = File::create(&temp_path)?;
        let mut buffer = [0u8; 8192];

        loop {
            let bytes_read = reader.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            temp_file.write_all(&buffer[..bytes_read])?;
        }
        temp_file.flush()?;
        drop(temp_file);

        fs::set_permissions(&temp_path, fs::Permissions::from_mode(0o755))?;
        fs::rename(&temp_path, &bin_path)?;

        println!("\x1b[1;32m[✓] cloudflared successfully installed at /usr/local/bin/cloudflared\x1b[0m");
        Ok(())
    }

    /// Detiene cualquier demonio de túnel (`tailscaled` o `cloudflared`) en ejecución
    pub fn stop(&self) -> Result<bool, DroidError> {
        let mut stopped_any = false;

        if let Some(pid) = self.tailscale_pid() {
            self.kill_process(pid);
            stopped_any = true;
        }
        let ts_pid_file = self.rootfs.join(TAILSCALE_PID_REL);
        if ts_pid_file.exists() {
            let _ = fs::remove_file(ts_pid_file);
        }

        if let Some(pid) = self.cloudflared_pid() {
            self.kill_process(pid);
            stopped_any = true;
        }
        let cf_pid_file = self.rootfs.join(CLOUDFLARED_PID_REL);
        if cf_pid_file.exists() {
            let _ = fs::remove_file(cf_pid_file);
        }

        Ok(stopped_any)
    }

    /// Consulta el estado actual del servicio de túneles
    pub fn status(&self) -> TunnelStatus {
        if let Some((provider, pid)) = self.active_daemon() {
            return TunnelStatus::Running { provider, pid };
        }

        if self.has_cloudflared_token() {
            TunnelStatus::Stopped {
                configured: Some(TunnelProvider::Cloudflared),
            }
        } else if self.is_tailscale_configured() {
            TunnelStatus::Stopped {
                configured: Some(TunnelProvider::Tailscale),
            }
        } else {
            TunnelStatus::NotConfigured
        }
    }

    /// Devuelve el proveedor activo y su PID si hay un túnel en ejecución
    pub fn active_daemon(&self) -> Option<(TunnelProvider, u32)> {
        if let Some(pid) = self.cloudflared_pid() {
            return Some((TunnelProvider::Cloudflared, pid));
        }
        if let Some(pid) = self.tailscale_pid() {
            return Some((TunnelProvider::Tailscale, pid));
        }
        None
    }

    /// Prepara el resolvedor DNS del host Android y garantiza que `/proc`, `/sys` y `/dev` estén montados
    fn prepare_runtime_environment(&self) -> Result<(), DroidError> {
        network::ensure_host_dns_resolver();
        let mount_mgr = MountManager::new(&self.rootfs);
        mount_mgr.mount_all()?;
        Ok(())
    }

    fn validate_cloudflared_token(token: &str) -> Result<(), DroidError> {
        if token.is_empty() {
            return Err(DroidError::Rootfs(
                "The Cloudflare Tunnel token cannot be empty".into(),
            ));
        }

        if !token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '=' || c == '.')
        {
            return Err(DroidError::Rootfs(
                "The Cloudflare Tunnel token contains invalid characters".into(),
            ));
        }

        Ok(())
    }

    fn is_tailscale_installed(&self) -> bool {
        self.rootfs
            .join(TAILSCALE_BIN_REL)
            .symlink_metadata()
            .is_ok()
    }

    fn is_tailscale_configured(&self) -> bool {
        self.rootfs.join(TAILSCALE_ENABLED_REL).exists()
            || self.rootfs.join(TAILSCALE_STATE_REL).exists()
    }

    fn is_cloudflared_installed(&self) -> bool {
        self.rootfs
            .join(CLOUDFLARED_BIN_REL)
            .symlink_metadata()
            .is_ok()
    }

    fn has_cloudflared_token(&self) -> bool {
        let path = self.rootfs.join(CLOUDFLARED_TOKEN_REL);
        fs::read_to_string(path)
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
    }

    fn tailscale_pid(&self) -> Option<u32> {
        self.resolve_daemon_pid(TAILSCALE_PID_REL, "tailscaled")
    }

    fn cloudflared_pid(&self) -> Option<u32> {
        self.resolve_daemon_pid(CLOUDFLARED_PID_REL, "cloudflared")
    }

    /// Verifica el archivo PID o localiza el proceso en `/proc` actualizando el archivo PID
    fn resolve_daemon_pid(&self, rel_path: &str, expected_process: &str) -> Option<u32> {
        let pid_file = self.rootfs.join(rel_path);

        if let Ok(raw_pid) = fs::read_to_string(&pid_file) {
            if let Ok(pid) = raw_pid.trim().parse::<u32>() {
                if Self::is_pid_matching_process(pid, expected_process) {
                    return Some(pid);
                }
            }
        }

        let pid = Self::find_pid_by_cmdline(expected_process)?;
        let _ = fs::write(&pid_file, format!("{}\n", pid));
        Some(pid)
    }

    /// Comprueba si `/proc/<pid>/cmdline` contiene el nombre del binario esperado
    fn is_pid_matching_process(pid: u32, expected_process: &str) -> bool {
        if pid == 0 {
            return false;
        }
        let proc_cmdline = PathBuf::from(format!("/proc/{}/cmdline", pid));
        let Ok(cmdline_bytes) = fs::read(proc_cmdline) else {
            return false;
        };
        let cmdline_lossy = String::from_utf8_lossy(&cmdline_bytes);
        cmdline_lossy.contains(expected_process)
    }

    /// Busca en `/proc` un proceso activo cuya línea de comandos coincida con el demonio
    fn find_pid_by_cmdline(expected_process: &str) -> Option<u32> {
        let entries = fs::read_dir("/proc").ok()?;
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let Some(name_str) = file_name.to_str() else {
                continue;
            };
            let Ok(pid) = name_str.parse::<u32>() else {
                continue;
            };
            if Self::is_pid_matching_process(pid, expected_process) {
                return Some(pid);
            }
        }
        None
    }

    fn kill_process(&self, pid: u32) {
        let pid_str = pid.to_string();
        let killed = Command::new("kill")
            .args(["-TERM", &pid_str])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if !killed {
            let runner = ChrootRunner::new(&self.rootfs);
            let _ = runner.run_command("/bin/kill", &["-TERM", &pid_str]);
        }
    }

    fn ensure_config_dir(&self) -> Result<(), DroidError> {
        let config_dir = self.rootfs.join("etc/droidvps");
        if !config_dir.exists() {
            fs::create_dir_all(&config_dir)?;
        }
        let log_dir = self.rootfs.join("var/log");
        if !log_dir.exists() {
            fs::create_dir_all(&log_dir)?;
        }
        let run_dir = self.rootfs.join("run");
        if !run_dir.exists() {
            fs::create_dir_all(&run_dir)?;
        }
        Ok(())
    }
}