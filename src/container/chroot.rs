use std::io;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use nix::unistd::{chdir, chroot, setsid};
use crate::error::DroidError;

const DEFAULT_PATH_ENV: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

/// Ejecutor de procesos aislados dentro del entorno `chroot` de Alpine Linux
pub struct ChrootRunner {
    rootfs: PathBuf,
}

impl ChrootRunner {
    /// Crea una nueva instancia de `ChrootRunner` apuntando al directorio `rootfs`
    pub fn new<P: AsRef<Path>>(rootfs: P) -> Self {
        Self {
            rootfs: rootfs.as_ref().to_path_buf(),
        }
    }

    /// Inicia una sesión de shell interactiva (`/bin/sh -l`) heredando la terminal actual
    pub fn run_shell(&self) -> Result<(), DroidError> {
        let rootfs_clone = self.rootfs.clone();

        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-l")
            .env_clear()
            .env("HOME", "/root")
            .env("USER", "root")
            .env("LOGNAME", "root")
            .env("TERM", "xterm-256color")
            .env("PATH", DEFAULT_PATH_ENV)
            .env("SHELL", "/bin/sh")
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        unsafe {
            cmd.pre_exec(move || {
                chroot(&rootfs_clone).map_err(|e| io::Error::from_raw_os_error(e as i32))?;
                chdir("/").map_err(|e| io::Error::from_raw_os_error(e as i32))?;
                Ok(())
            });
        }

        let status = cmd.status()?;
        if !status.success() {
            // En una sesión interactiva, salir con el último código de retorno de un comando
            // dentro de la shell es habitual, por lo que solo fallamos si fue terminado por señal crítica
            if status.code().is_none() {
                return Err(DroidError::Rootfs(
                    "The interactive chroot session terminated abruptly due to a signal".into(),
                ));
            }
        }

        Ok(())
    }

    /// Ejecuta un comando no interactivo de forma síncrona dentro del `chroot`
    pub fn run_command(&self, program: &str, args: &[&str]) -> Result<(), DroidError> {
        let rootfs_clone = self.rootfs.clone();

        let mut cmd = Command::new(program);
        cmd.args(args)
            .env_clear()
            .env("HOME", "/root")
            .env("USER", "root")
            .env("TERM", "xterm-256color")
            .env("PATH", DEFAULT_PATH_ENV)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        unsafe {
            cmd.pre_exec(move || {
                chroot(&rootfs_clone).map_err(|e| io::Error::from_raw_os_error(e as i32))?;
                chdir("/").map_err(|e| io::Error::from_raw_os_error(e as i32))?;
                Ok(())
            });
        }

        let status = cmd.status()?;
        if !status.success() {
            return Err(DroidError::Rootfs(format!(
                "Command '{} {}' inside the chroot failed with status: {}",
                program,
                args.join(" "),
                status
            )));
        }

        Ok(())
    }

    /// Inicia un proceso demonio en segundo plano desacoplado de la terminal (`setsid`)
    pub fn spawn_daemon(&self, program: &str, args: &[&str]) -> Result<(), DroidError> {
        let rootfs_clone = self.rootfs.clone();

        let mut cmd = Command::new(program);
        cmd.args(args)
            .env_clear()
            .env("HOME", "/root")
            .env("USER", "root")
            .env("PATH", DEFAULT_PATH_ENV)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        unsafe {
            cmd.pre_exec(move || {
                // Crear una nueva sesión para desacoplar el demonio de la terminal de control
                let _ = setsid();
                chroot(&rootfs_clone).map_err(|e| io::Error::from_raw_os_error(e as i32))?;
                chdir("/").map_err(|e| io::Error::from_raw_os_error(e as i32))?;
                Ok(())
            });
        }

        let mut child = cmd.spawn()?;

        // Si el proceso es un demonio que hace fork (ej. Dropbear sin -F), esperamos brevemente
        // o verificamos que no haya fallado de inmediato al arrancar.
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                return Err(DroidError::Rootfs(format!(
                    "Daemon '{}' exited prematurely with error: {}",
                    program, status
                )));
            }
        }

        Ok(())
    }
}