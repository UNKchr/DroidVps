use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;
use crate::container::chroot::ChrootRunner;
use crate::error::DroidError;

const TAILSCALE_BIN_REL: &str = "usr/sbin/tailscaled";
const TAILSCALE_CLI_IN_CHROOT: &str = "/usr/bin/tailscale";
const TAILSCALE_PID_REL: &str = "run/tailscaled.pid";
const TAILSCALE_SOCKET_ARG: &str = "--socket=/run/tailscale/tailscaled.sock";
const TAILSCALE_STATE_REL: &str = "var/lib/tailscale/tailscaled.state";
const TAILSCALE_ENABLED_REL: &str = "etc/droidvps/tailscale.enabled";

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

    /// Flujo asistido para Tailscale: instala el paquete si falta, inicia `tailscaled`
    /// y ejecuta `tailscale up` (con auth_key opcional o mostrando el enlace interactivo).
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

    /// Instala (si es necesario) e inicia el demonio `tailscaled` en modo userspace networking
    pub fn start_tailscale(&self) -> Result<u32, DroidError> {
        if let Some(pid) = self.tailscale_pid() {
            return Ok(pid);
        }

        let runner = ChrootRunner::new(&self.rootfs);

        if !self.is_tailscale_installed() {
            println!("[*] Tailscale not detected. Installing automatically via apk...");
            runner.run_command("/sbin/apk", &["update"])?;
            runner.run_command("/sbin/apk", &["add", "--no-cache", "tailscale"])?;
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

        runner.spawn_daemon(
            "/usr/sbin/tailscaled",
            &[
                "--tun=userspace-networking",
                "--statedir=/var/lib/tailscale",
                TAILSCALE_SOCKET_ARG,
            ],
        )?;

        for _ in 0..20 {
            if let Some(pid) = self.tailscale_pid() {
                return Ok(pid);
            }
            thread::sleep(Duration::from_millis(50));
        }

        Err(DroidError::Rootfs(
            "tailscaled ran but no active process was detected in /proc".into(),
        ))
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

        // Auto-instalación inteligente si el binario no existe en el contenedor
        self.ensure_cloudflared_installed()?;
        self.ensure_config_dir()?;

        let runner = ChrootRunner::new(&self.rootfs);
        runner.spawn_daemon(
            "/usr/local/bin/cloudflared",
            &[
                "tunnel",
                "--no-autoupdate",
                "--pidfile",
                "/run/cloudflared.pid",
                "--logfile",
                "/var/log/cloudflared.log",
                "run",
                "--token",
                &token,
            ],
        )?;

        for _ in 0..20 {
            if let Some(pid) = self.cloudflared_pid() {
                return Ok(pid);
            }
            thread::sleep(Duration::from_millis(50));
        }

        Err(DroidError::Rootfs(
            "cloudflared ran but did not register an active PID in /proc".into(),
        ))
    }

    /// Garantiza que el binario `/usr/local/bin/cloudflared` (`aarch64`) esté instalado.
    /// Si no existe, lo descarga por streaming HTTP con `ureq` y le asigna permisos `0755`.
    pub fn ensure_cloudflared_installed(&self) -> Result<(), DroidError> {
        if self.is_cloudflared_installed() {
            return Ok(());
        }

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
        self.rootfs.join(TAILSCALE_BIN_REL).exists()
    }

    fn is_tailscale_configured(&self) -> bool {
        self.rootfs.join(TAILSCALE_ENABLED_REL).exists()
            || self.rootfs.join(TAILSCALE_STATE_REL).exists()
    }

    fn is_cloudflared_installed(&self) -> bool {
        self.rootfs.join(CLOUDFLARED_BIN_REL).exists()
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