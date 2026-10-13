use std::fs;
use std::path::{Path, PathBuf};
use nix::mount::{mount, umount2, MntFlags, MsFlags};
use crate::error::DroidError;

const PROC_MOUNTS_PATH: &str = "/proc/mounts";

/// Gestiona los montajes de pseudo-sistemas de archivos del kernel dentro del rootfs
pub struct MountManager {
    rootfs: PathBuf,
}

impl MountManager {
    /// Crea una nueva instancia del gestor de montajes para la ruta rootfs indicada
    pub fn new<P: AsRef<Path>>(rootfs: P) -> Self {
        Self {
            rootfs: rootfs.as_ref().to_path_buf(),
        }
    }

    /// Monta de forma idempotente `/dev`, `/dev/pts`, `/proc` y `/sys` dentro del contenedor
    pub fn mount_all(&self) -> Result<(), DroidError> {
        let dev_target = self.rootfs.join("dev");
        let dev_pts_target = self.rootfs.join("dev/pts");
        let dev_shm_target = self.rootfs.join("dev/shm");
        let proc_target = self.rootfs.join("proc");
        let sys_target = self.rootfs.join("sys");

        // Asegurar que los directorios base existen antes de invocar mount(2)
        for dir in [&dev_target, &proc_target, &sys_target] {
            if !dir.exists() {
                fs::create_dir_all(dir)?;
            }
        }

        // 1. Bind mount de /dev + esclavización de propagación (MS_SLAVE)
        if !self.is_target_mounted(&dev_target) {
            mount(
                Some(Path::new("/dev")),
                &dev_target,
                None::<&str>,
                MsFlags::MS_BIND | MsFlags::MS_REC,
                None::<&str>,
            )?;
            let _ = mount(
                None::<&str>,
                &dev_target,
                None::<&str>,
                MsFlags::MS_SLAVE | MsFlags::MS_REC,
                None::<&str>,
            );
        }

        // 2. Bind mount de /dev/pts (esencial para asignación de PTY en Dropbear SSH)
        if !dev_pts_target.exists() {
            let _ = fs::create_dir_all(&dev_pts_target);
        }
        if !self.is_target_mounted(&dev_pts_target) {
            mount(
                Some(Path::new("/dev/pts")),
                &dev_pts_target,
                None::<&str>,
                MsFlags::MS_BIND,
                None::<&str>,
            )?;
        }

        // Asegurar directorio /dev/shm para memoria compartida POSIX
        if !dev_shm_target.exists() {
            let _ = fs::create_dir_all(&dev_shm_target);
        }

        // 3. Montaje de pseudo-filesystem /proc
        if !self.is_target_mounted(&proc_target) {
            mount(
                Some("proc"),
                &proc_target,
                Some("proc"),
                MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_NOEXEC,
                None::<&str>,
            )?;
        }

        // 4. Bind mount de /sys + esclavización de propagación
        if !self.is_target_mounted(&sys_target) {
            mount(
                Some(Path::new("/sys")),
                &sys_target,
                None::<&str>,
                MsFlags::MS_BIND | MsFlags::MS_REC,
                None::<&str>,
            )?;
            let _ = mount(
                None::<&str>,
                &sys_target,
                None::<&str>,
                MsFlags::MS_SLAVE | MsFlags::MS_REC,
                None::<&str>,
            );
        }

        Ok(())
    }

    /// Desmonta de forma segura (`MNT_DETACH`) los sistemas de archivos en orden inverso
    pub fn unmount_all(&self) -> Result<(), DroidError> {
        let targets = [
            self.rootfs.join("sys"),
            self.rootfs.join("proc"),
            self.rootfs.join("dev/pts"),
            self.rootfs.join("dev"),
        ];

        let mut last_error: Option<DroidError> = None;

        for target in &targets {
            if self.is_target_mounted(target) {
                if let Err(err) = umount2(target, MntFlags::MNT_DETACH) {
                    last_error = Some(DroidError::Nix(err));
                }
            }
        }

        if let Some(err) = last_error {
            return Err(err);
        }

        Ok(())
    }

    /// Devuelve `true` si al menos uno de los subsistemas clave del contenedor está montado
    pub fn is_mounted(&self) -> bool {
        !self.active_mounts().is_empty()
    }

    /// Lista los puntos de montaje relativos activos dentro del rootfs según `/proc/mounts`
    pub fn active_mounts(&self) -> Vec<String> {
        let candidates = ["dev", "dev/pts", "proc", "sys"];
        let mut active = Vec::new();

        for rel in candidates {
            let full_path = self.rootfs.join(rel);
            if self.is_target_mounted(&full_path) {
                active.push(format!("/{}", rel));
            }
        }

        active
    }

    /// Verifica en `/proc/mounts` si una ruta específica ya se encuentra montada
    fn is_target_mounted(&self, target: &Path) -> bool {
        let Ok(mounts_content) = fs::read_to_string(PROC_MOUNTS_PATH) else {
            return false;
        };

        let canonical_target = fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf());
        let target_str = canonical_target.to_string_lossy();
        let raw_target_str = target.to_string_lossy();

        for line in mounts_content.lines() {
            let mut parts = line.split_whitespace();
            let _source = parts.next();
            if let Some(mount_point) = parts.next() {
                if mount_point == target_str || mount_point == raw_target_str {
                    return true;
                }
            }
        }

        false
    }
}