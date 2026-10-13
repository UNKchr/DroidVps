use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;
use crate::container::chroot::ChrootRunner;
use crate::error::DroidError;

/// Puerto TCP por defecto para Dropbear SSH (evita colisiones con demonios del host Android)
pub const DEFAULT_SSH_PORT: u16 = 2222;

/// Contraseña inicial configurada para `root` en el primer aprovisionamiento
pub const DEFAULT_ROOT_PASSWORD: &str = "droidvps";

const DROPBEAR_BIN_REL: &str = "usr/sbin/dropbear";
const DROPBEAR_PID_REL: &str = "run/dropbear.pid";
const SSH_INIT_MARKER_REL: &str = "etc/dropbear/.droidvps_initialized";

/// Orquestador del servicio Dropbear SSH dentro del contenedor Alpine Linux
pub struct SshService {
    rootfs: PathBuf,
    port: u16,
}

impl SshService {
    /// Crea un gestor del servicio SSH con el puerto por defecto (`2222`)
    pub fn new<P: AsRef<Path>>(rootfs: P) -> Self {
        Self {
            rootfs: rootfs.as_ref().to_path_buf(),
            port: DEFAULT_SSH_PORT,
        }
    }

    /// Crea un gestor del servicio SSH especificando un puerto TCP personalizado
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

    /// Comprueba si el binario de Dropbear ya está instalado dentro del `rootfs`
    pub fn is_installed(&self) -> bool {
        self.rootfs.join(DROPBEAR_BIN_REL).exists()
    }

    /// Garantiza que Dropbear esté instalado, los grupos de red de Android estén registrados
    /// en `/etc/group` y las credenciales iniciales de `root` estén listas
    pub fn ensure_installed(&self) -> Result<(), DroidError> {
        self.ensure_android_network_groups()?;

        let runner = ChrootRunner::new(&self.rootfs);

        if !self.is_installed() {
            println!("[*] Instalando Dropbear SSH dentro del contenedor Alpine...");
            runner.run_command("/sbin/apk", &["update"])?;
            runner.run_command(
                "/sbin/apk",
                &["add", "--no-cache", "dropbear", "dropbear-scp"],
            )?;
        }

        // Asegurar directorios críticos con permisos restrictivos
        let dropbear_etc = self.rootfs.join("etc/dropbear");
        let root_ssh_dir = self.rootfs.join("root/.ssh");
        let run_dir = self.rootfs.join("run");

        for dir in [&dropbear_etc, &root_ssh_dir, &run_dir] {
            if !dir.exists() {
                fs::create_dir_all(dir)?;
            }
        }

        fs::set_permissions(&dropbear_etc, fs::Permissions::from_mode(0o700))?;
        fs::set_permissions(&root_ssh_dir, fs::Permissions::from_mode(0o700))?;

        // Configurar contraseña inicial de root solo la primera vez
        let marker_path = self.rootfs.join(SSH_INIT_MARKER_REL);
        if !marker_path.exists() {
            let chpasswd_cmd = format!(
                "echo 'root:{}' | /usr/sbin/chpasswd",
                DEFAULT_ROOT_PASSWORD
            );
            runner.run_command("/bin/sh", &["-c", &chpasswd_cmd])?;
            fs::write(&marker_path, b"initialized\n")?;
        }

        Ok(())
    }

    /// Inicia el demonio Dropbear SSH en segundo plano y devuelve su PID activo
    pub fn start(&self) -> Result<u32, DroidError> {
        if let Some(existing_pid) = self.running_pid() {
            return Ok(existing_pid);
        }

        self.ensure_installed()?;

        // Limpiar archivo PID obsoleto si existiera de una ejecución previa
        let pid_file = self.rootfs.join(DROPBEAR_PID_REL);
        if pid_file.exists() {
            let _ = fs::remove_file(&pid_file);
        }

        let port_arg = self.port.to_string();
        let runner = ChrootRunner::new(&self.rootfs);

        // -R: Generar hostkeys automáticamente (ed25519/rsa/ecdsa) si no existen
        // -p: Puerto de escucha TCP
        // -P: Ruta del archivo PID dentro del chroot
        runner.spawn_daemon(
            "/usr/sbin/dropbear",
            &["-R", "-p", &port_arg, "-P", "/run/dropbear.pid"],
        )?;

        // Esperar hasta 1 segundo (20 intentos de 50ms) a que Dropbear escriba su PID
        for _ in 0..20 {
            if let Some(pid) = self.running_pid() {
                return Ok(pid);
            }
            thread::sleep(Duration::from_millis(50));
        }

        Err(DroidError::Rootfs(
            "Dropbear se ejecutó pero no registró un PID activo en /run/dropbear.pid".into(),
        ))
    }

    /// Detiene el demonio Dropbear SSH si se encuentra en ejecución
    pub fn stop(&self) -> Result<bool, DroidError> {
        let pid_file = self.rootfs.join(DROPBEAR_PID_REL);

        let Some(pid) = self.running_pid() else {
            if pid_file.exists() {
                let _ = fs::remove_file(&pid_file);
            }
            return Ok(false);
        };

        let pid_str = pid.to_string();

        // Intentar terminar el proceso de forma limpia mediante SIGTERM
        let terminated = Command::new("kill")
            .args(["-TERM", &pid_str])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if !terminated {
            let runner = ChrootRunner::new(&self.rootfs);
            let _ = runner.run_command("/bin/kill", &["-TERM", &pid_str]);
        }

        if pid_file.exists() {
            let _ = fs::remove_file(&pid_file);
        }

        Ok(true)
    }

    /// Devuelve `true` si el demonio Dropbear SSH está activo en memoria
    pub fn is_running(&self) -> bool {
        self.running_pid().is_some()
    }

    /// Obtiene el PID de Dropbear validando en `/proc/<pid>/cmdline` que el proceso exista
    pub fn running_pid(&self) -> Option<u32> {
        let pid_file = self.rootfs.join(DROPBEAR_PID_REL);
        let raw_pid = fs::read_to_string(pid_file).ok()?;
        let pid = raw_pid.trim().parse::<u32>().ok()?;

        if pid == 0 {
            return None;
        }

        let proc_cmdline = PathBuf::from(format!("/proc/{}/cmdline", pid));
        let cmdline_bytes = fs::read(proc_cmdline).ok()?;
        let cmdline_lossy = String::from_utf8_lossy(&cmdline_bytes);

        if cmdline_lossy.contains("dropbear") {
            Some(pid)
        } else {
            None
        }
    }

    /// Registra `aid_inet` (3003) y `aid_net_raw` (3004) en `<rootfs>/etc/group`
    /// para que las sesiones SSH de `root` conserven permisos de sockets en el kernel Android
    fn ensure_android_network_groups(&self) -> Result<(), DroidError> {
        let group_path = self.rootfs.join("etc/group");
        if !group_path.exists() {
            return Ok(());
        }

        let mut content = fs::read_to_string(&group_path)?;
        let mut modified = false;

        if !content.contains(":3003:") {
            if !content.ends_with('\n') {
                content.push('\n');
            }
            content.push_str("aid_inet:x:3003:root\n");
            modified = true;
        }

        if !content.contains(":3004:") {
            if !content.ends_with('\n') {
                content.push('\n');
            }
            content.push_str("aid_net_raw:x:3004:root\n");
            modified = true;
        }

        if modified {
            fs::write(&group_path, content)?;
        }

        Ok(())
    }
}