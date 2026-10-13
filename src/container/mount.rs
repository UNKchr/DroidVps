use crate::error::DroidError;
use nix::mount::{mount, umount2, MntFlags, MsFlags};
use std::fs;
use std::path::{Path, PathBuf};

pub struct MountManager {
    rootfs_path: PathBuf,
}

impl MountManager {
    pub fn new(rootfs_path: &Path) -> Self {
        Self {
            rootfs_path: rootfs_path.to_path_buf(),
        }
    }

    /// Realiza los bind mounts necesarios para que el entorno de Alpine 
    /// interactúe correctamente con el kernel de Android.
    pub fn mount_all(&self) -> Result<(), DroidError> {
        // Tupla: (Origen en Android, Destino en Rootfs, Flags de montaje)
        let mounts = [
            // MS_REC es clave en /dev para montar recursivamente sub-montajes si existen
            ("/dev", "dev", MsFlags::MS_BIND | MsFlags::MS_REC),
            ("/dev/pts", "dev/pts", MsFlags::MS_BIND),
            ("/proc", "proc", MsFlags::MS_BIND),
            ("/sys", "sys", MsFlags::MS_BIND),
        ];

        for (src, dest, flags) in mounts.iter() {
            let target = self.rootfs_path.join(dest);
            
            // Garantizamos que el punto de montaje exista en el contenedor
            if !target.exists() {
                fs::create_dir_all(&target)?;
            }

            mount(
                Some(*src),
                &target,
                None::<&str>, // El fstype es ignorado en bind mounts
                *flags,
                None::<&str>, // Sin opciones extra de datos
            ).map_err(|e| {
                DroidError::Rootfs(format!("Error during installation {} in {:?}: {}", src, target, e))
            })?;
        }

        Ok(())
    }

    /// Revierte los montajes de forma segura para no dejar inconsistente a Android.
    pub fn unmount_all(&self) -> Result<(), DroidError> {
        // El orden inverso es obligatorio. Desmontar /dev antes que /dev/pts 
        // dejaría el filesystem bloqueado (Device or resource busy).
        let mounts = ["sys", "proc", "dev/pts", "dev"];

        for dest in mounts.iter() {
            let target = self.rootfs_path.join(dest);
            
            if target.exists() {
                // MNT_DETACH (lazy unmount) es crucial. Si un proceso de Alpine 
                // se quedó zombie (ej. un daemon en background), un desmontaje normal fallaría.
                // Detach obliga al VFS del kernel a desconectar el punto inmediatamente.
                if let Err(e) = umount2(&target, MntFlags::MNT_DETACH) {
                    // Ignoramos EINVAL (22) que significa que ya estaba desmontado o no es un mountpoint
                    if e != nix::errno::Errno::EINVAL {
                        return Err(DroidError::Rootfs(format!(
                            "Critical error during disassembly {:?}: {}", target, e
                        )));
                    }
                }
            }
        }

        Ok(())
    }
}