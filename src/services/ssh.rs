use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;
use crate::container::chroot::ChrootRunner;
use crate::error::DroidError;
use crate::rootfs::configure_android_groups;

/// Puerto TCP por defecto para Dropbear SSH (evita colisiones con servicios del host Android)
pub const DEFAULT_SSH_PORT: u16 = 2222;

/// Contraseña inicial configurada automáticamente para el usuario `root` en el primer aprovisionamiento
pub const DEFAULT_ROOT_PASSWORD: &str = "droidvps";

const DROPBEAR_BIN_REL: &str = "usr/sbin/dropbear";
const DROPBEAR_PID_REL: &str = "run/dropbear.pid";
const AUTH_INITIALIZED_MARKER: &str = "etc/dropbear/.droidvps_auth_ready";

/// Orquestador del servicio Dropbear SSH dentro del contenedor Alpine Linux
pub struct SshService {
    rootfs: PathBuf,
    port: u16,
}

impl SshService {
    /// Crea una nueva instancia del gestor SSH con el puerto por defecto (`2222`)
    pub fn new<P: AsRef<Path>>(rootfs: P) -> Self {
        Self {
            rootfs: rootfs.as_ref().to_path_buf(),
            port: DEFAULT_SSH_PORT,
        }
    }

    /// Crea una instancia especificando un puerto TCP personalizado
    pub fn with_port<P: AsRef<Path>>(rootfs: P, port: u16) -> Self {
        Self {
            rootfs: rootfs.as_ref().to_path_buf(),
            port,
        }
    }

    /// Devuelve el puerto TCP configurado para el servidor SSH
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Verifica si el binario `/usr/sbin/dropbear` existe dentro del `rootfs` usando `symlink_metadata`
    /// para soportar enlaces simbólicos absolutos (`/usr/sbin/dropbear -> /usr/bin/dropbearmulti`)
    pub fn is_installed(&self) -> bool {
        self.rootfs
            .join(DROPBEAR_BIN_REL)
            .symlink_metadata()
            .is_ok()
    }

    /// Garantiza que Dropbear esté instalado, los grupos de red Android estén registrados
    /// y las credenciales iniciales de `root` estén listas
    pub fn ensure_installed(&self) -> Result<(), DroidError> {
        let runner = ChrootRunner::new(&self.rootfs);

        if !self.is_installed() {
            println!("[*] Dropbear SSH not found. Installing automatically via apk...");
            runner.run_command("/sbin/apk", &["update"])?;
            runner.run_command("/sbin/apk", &["add", "--no-cache", "dropbear"])?;
        }

        // Asegurar directorios requeridos con permisos restrictivos
        let dropbear_etc = self.rootfs.join("etc/dropbear");
        if !dropbear_etc.exists() {
            fs::create_dir_all(&dropbear_etc)?;
        }
        fs::set_permissions(&dropbear_etc, fs::Permissions::from_mode(0o700))?;

        let run_dir = self.rootfs.join("run");
        if !run_dir.exists() {
            fs::create_dir_all(&run_dir)?;
        }

        let root_ssh = self.rootfs.join("root/.ssh");
        if !root_ssh.exists() {
            fs::create_dir_all(&root_ssh)?;
        }
        fs::set_permissions(&root_ssh, fs::Permissions::from_mode(0o700))?;

        // Garantizar que los grupos de red de Android (3003/3004) existan en /etc/group
        configure_android_groups(&self.rootfs)?;

        // Configurar contraseña inicial de root una sola vez
        let marker = self.rootfs.join(AUTH_INITIALIZED_MARKER);
        if !marker.exists() {
            let chpasswd_cmd = format!(
                "echo 'root:{}' | /usr/sbin/chpasswd",
                DEFAULT_ROOT_PASSWORD
            );
            runner.run_command("/bin/sh", &["-c", &chpasswd_cmd])?;
            fs::write(&marker, b"initialized\n")?;
            fs::set_permissions(&marker, fs::Permissions::from_mode(0o600))?;
        }

        Ok(())
    }

    /// Instala (si falta) e inicia el demonio Dropbear SSH en segundo plano devolviendo su PID activo
    pub fn start(&self) -> Result<u32, DroidError> {
        if let Some(pid) = self.running_pid() {
            return Ok(pid);
        }

        self.ensure_installed()?;

        // Limpiar archivo PID obsoleto si existiera de un apagado abrupto previo
        let pid_file = self.rootfs.join(DROPBEAR_PID_REL);
        if pid_file.exists() {
            let _ = fs::remove_file(&pid_file);
        }

        let port_arg = self.port.to_string();
        let runner = ChrootRunner::new(&self.rootfs);

        // -R: genera hostkeys automáticamente en /etc/dropbear si no existen
        // -E: envía logs a stderr
        // -p: puerto TCP de escucha
        // -P: ruta del archivo PID dentro del chroot
        runner.spawn_daemon(
            "/usr/sbin/dropbear",
            &["-R", "-E", "-p", &port_arg, "-P", "/run/dropbear.pid"],
        )?;

        for _ in 0..25 {
            if let Some(pid) = self.running_pid() {
                return Ok(pid);
            }
            thread::sleep(Duration::from_millis(60));
        }

        Err(DroidError::Rootfs(
            "Dropbear SSH daemon spawned, but no active process was detected in /proc".into(),
        ))
    }

    /// Detiene el demonio Dropbear SSH si se encuentra en ejecución
    pub fn stop(&self) -> Result<bool, DroidError> {
        let Some(pid) = self.running_pid() else {
            let pid_file = self.rootfs.join(DROPBEAR_PID_REL);
            if pid_file.exists() {
                let _ = fs::remove_file(pid_file);
            }
            return Ok(false);
        };

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

        let pid_file = self.rootfs.join(DROPBEAR_PID_REL);
        if pid_file.exists() {
            let _ = fs::remove_file(pid_file);
        }

        Ok(true)
    }

    /// Devuelve `true` si el demonio Dropbear SSH está activo
    pub fn is_running(&self) -> bool {
        self.running_pid().is_some()
    }

    /// Obtiene el PID real de Dropbear validando `/run/dropbear.pid` contra `/proc/<pid>/cmdline`
    pub fn running_pid(&self) -> Option<u32> {
        let pid_file = self.rootfs.join(DROPBEAR_PID_REL);

        if let Ok(raw_pid) = fs::read_to_string(&pid_file) {
            if let Ok(pid) = raw_pid.trim().parse::<u32>() {
                if Self::is_dropbear_process(pid) {
                    return Some(pid);
                }
            }
        }

        let pid = Self::find_dropbear_in_proc()?;
        let _ = fs::write(&pid_file, format!("{}\n", pid));
        Some(pid)
    }

    fn is_dropbear_process(pid: u32) -> bool {
        if pid == 0 {
            return false;
        }
        let cmdline_path = PathBuf::from(format!("/proc/{}/cmdline", pid));
        let Ok(bytes) = fs::read(cmdline_path) else {
            return false;
        };
        String::from_utf8_lossy(&bytes).contains("dropbear")
    }

    fn find_dropbear_in_proc() -> Option<u32> {
        let entries = fs::read_dir("/proc").ok()?;
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let Some(name_str) = file_name.to_str() else {
                continue;
            };
            let Ok(pid) = name_str.parse::<u32>() else {
                continue;
            };
            if Self::is_dropbear_process(pid) {
                return Some(pid);
            }
        }
        None
    }
}