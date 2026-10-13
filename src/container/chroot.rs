use crate::error::DroidError;
use nix::unistd::{chdir, chroot};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct ChrootRunner {
    rootfs_path: PathBuf,
}

impl ChrootRunner {
    pub fn new(rootfs_path: &Path) -> Self {
        Self {
            rootfs_path: rootfs_path.to_path_buf(),
        }
    }

    /// Spawnea una shell interactiva dentro del entorno aislado de Alpine.
    pub fn run_shell(&self) -> Result<(), DroidError> {
        let root_dir = self.rootfs_path.clone();
        let mut cmd = Command::new("/bin/sh");

        // 1. Limpiamos las variables de Android (Bionic) e inyectamos las de Linux estándar
        cmd.env_clear()
            .env("PATH", "/bin:/sbin:/usr/bin:/usr/sbin")
            .env("HOME", "/root")
            .env("USER", "root")
            .env("SHELL", "/bin/sh")
            .env("TERM", std::env::var("TERM").unwrap_or_else(|_| "xterm-256color".to_string()));

        // 2. Ejecutar chroot y chdir de forma atómica en el proceso hijo justo antes de 
        // reemplazar la memoria con /bin/sh (execv).
        unsafe {
            cmd.pre_exec(move || {
                chroot(&root_dir).map_err(std::io::Error::other)?;
                chdir("/").map_err(std::io::Error::other)?;
                Ok(())
            });
        }

        // 3. Ejecutamos la shell de forma interactiva y bloqueante.
        // Rust cede el stdin, stdout y stderr automáticamente.
        let status = cmd.status()?;

        if !status.success() {
            // El usuario o la shell finalizó con error, lo registramos si usáramos un logger,
            // pero NO retornamos Err() para no truncar la limpieza en `container.rs`.
        }

        Ok(())
    }
}