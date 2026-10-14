use std::fs;
use std::path::{Path, PathBuf};
use nix::mount::{mount, umount2, MntFlags, MsFlags};
use crate::error::DroidError;

/// Orden estricto de montaje de los subsistemas del kernel
const MOUNT_POINTS: [&str; 4] = ["dev", "dev/pts", "proc", "sys"];

/// Gestor idempotente de puntos de montaje para el contenedor `chroot`
pub struct MountManager {
    rootfs: PathBuf,
}

impl MountManager {
    /// Crea una nueva instancia del gestor de montajes asociada al `rootfs` indicado
    pub fn new<P: AsRef<Path>>(rootfs: P) -> Self {
        Self {
            rootfs: rootfs.as_ref().to_path_buf(),
        }
    }

    /// Monta de forma idempotente `/dev`, `/dev/pts`, `/proc` y `/sys` dentro del `rootfs`
    pub fn mount_all(&self) -> Result<(), DroidError> {
        // 1. Bind mount recursivo de /dev
        self.mount_bind("/dev", "dev")?;

        // 2. Asegurar /dev/pts (si el bind recursivo de /dev aún no lo montó)
        self.mount_devpts("dev/pts")?;

        // 3. Montar /proc (con fallback a bind mount si el kernel restringe nuevas instancias proc)
        self.mount_proc("proc")?;

        // 4. Montar /sys (con fallback a bind mount si el kernel restringe nuevas instancias sysfs)
        self.mount_sys("sys")?;

        Ok(())
    }

    /// Desmonta de forma perezosa (`MNT_DETACH`) todos los subsistemas en orden inverso
    pub fn unmount_all(&self) -> Result<(), DroidError> {
        for rel_path in MOUNT_POINTS.iter().rev() {
            let target = self.rootfs.join(rel_path);
            if self.is_mounted(&target)? {
                umount2(&target, MntFlags::MNT_DETACH)?;
            }
        }
        Ok(())
    }

    /// Devuelve la lista de puntos de montaje relativos que se encuentran activos en `/proc/mounts`
    pub fn active_mounts(&self) -> Vec<String> {
        let mut active = Vec::new();
        for rel_path in MOUNT_POINTS {
            let target = self.rootfs.join(rel_path);
            if self.is_mounted(&target).unwrap_or(false) {
                active.push(format!("/{}", rel_path));
            }
        }
        active
    }

    /// Ejecuta un bind mount recursivo (`MS_BIND | MS_REC`) y aplica `MS_SLAVE` de forma best-effort
    fn mount_bind(&self, source: &str, rel_target: &str) -> Result<(), DroidError> {
        let target = self.rootfs.join(rel_target);
        if !target.exists() {
            fs::create_dir_all(&target)?;
        }

        if self.is_mounted(&target)? {
            return Ok(());
        }

        mount(
            Some(source),
            &target,
            None::<&str>,
            MsFlags::MS_BIND | MsFlags::MS_REC,
            None::<&str>,
        )?;

        // En Android, si el montaje origen es MS_PRIVATE, MS_SLAVE devuelve EINVAL (22).
        // Se ignora ese retorno porque un montaje MS_PRIVATE ya está aislado por definición.
        let _ = mount(
            None::<&str>,
            &target,
            None::<&str>,
            MsFlags::MS_SLAVE | MsFlags::MS_REC,
            None::<&str>,
        );

        Ok(())
    }

    /// Monta `/dev/pts` como `devpts` o reutiliza `/dev/pts` del host mediante `MS_BIND`
    fn mount_devpts(&self, rel_target: &str) -> Result<(), DroidError> {
        let target = self.rootfs.join(rel_target);
        if !target.exists() {
            fs::create_dir_all(&target)?;
        }

        // Si el bind recursivo de /dev ya montó /dev/pts o expuso ptmx, no re-montar
        if self.is_mounted(&target)? || target.join("ptmx").exists() {
            return Ok(());
        }

        let res = mount(
            Some("devpts"),
            &target,
            Some("devpts"),
            MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC,
            None::<&str>,
        );

        if res.is_err() {
            mount(
                Some("/dev/pts"),
                &target,
                None::<&str>,
                MsFlags::MS_BIND | MsFlags::MS_REC,
                None::<&str>,
            )?;
        }

        Ok(())
    }

    /// Monta `/proc` virtual o realiza bind mount desde `/proc` del host como respaldo
    fn mount_proc(&self, rel_target: &str) -> Result<(), DroidError> {
        let target = self.rootfs.join(rel_target);
        if !target.exists() {
            fs::create_dir_all(&target)?;
        }

        if self.is_mounted(&target)? {
            return Ok(());
        }

        let res = mount(
            Some("proc"),
            &target,
            Some("proc"),
            MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_NOEXEC,
            None::<&str>,
        );

        if res.is_err() {
            mount(
                Some("/proc"),
                &target,
                None::<&str>,
                MsFlags::MS_BIND | MsFlags::MS_REC,
                None::<&str>,
            )?;
        }

        Ok(())
    }

    /// Monta `/sys` virtual o realiza bind mount desde `/sys` del host como respaldo
    fn mount_sys(&self, rel_target: &str) -> Result<(), DroidError> {
        let target = self.rootfs.join(rel_target);
        if !target.exists() {
            fs::create_dir_all(&target)?;
        }

        if self.is_mounted(&target)? {
            return Ok(());
        }

        let res = mount(
            Some("sysfs"),
            &target,
            Some("sysfs"),
            MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_NOEXEC,
            None::<&str>,
        );

        if res.is_err() {
            self.mount_bind("/sys", rel_target)?;
        }

        Ok(())
    }

    /// Verifica en `/proc/mounts` si la ruta objetivo ya figura como punto de montaje activo
    pub fn is_mounted(&self, target: &Path) -> Result<bool, DroidError> {
        let mounts_content = fs::read_to_string("/proc/mounts")?;
        let canonical_target = fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf());
        let target_str = canonical_target.to_string_lossy();
        let raw_target_str = target.to_string_lossy();

        for line in mounts_content.lines() {
            let mut parts = line.split_whitespace();
            let _device = parts.next();
            if let Some(mount_point) = parts.next() {
                if mount_point == target_str || mount_point == raw_target_str {
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }
}