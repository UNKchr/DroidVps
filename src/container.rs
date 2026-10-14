pub mod chroot;
pub mod mount;

use std::path::Path;
use crate::android::{network, power, selinux};
use crate::container::chroot::ChrootRunner;
use crate::container::mount::MountManager;
use crate::error::DroidError;
use crate::rootfs;
use crate::services::{SshService, TunnelService, TunnelStatus};

/// Resumen del estado tras arrancar los servicios del nodo VPS en segundo plano
#[derive(Debug, Clone)]
pub struct VpsStartReport {
    pub ssh_pid: u32,
    pub ssh_port: u16,
    pub tunnel_status: TunnelStatus,
}

/// Resumen de las acciones realizadas al detener el nodo VPS
#[derive(Debug, Clone)]
pub struct VpsStopReport {
    pub ssh_stopped: bool,
    pub tunnel_stopped: bool,
    pub filesystems_unmounted: bool,
}

/// Estado operativo integral del contenedor y sus servicios asociados
#[derive(Debug, Clone)]
pub struct VpsStatus {
    pub rootfs_initialized: bool,
    pub active_mounts: Vec<String>,
    pub ssh_pid: Option<u32>,
    pub ssh_port: u16,
    pub tunnel_status: TunnelStatus,
}

/// Prepara los subsistemas del kernel Android, monta pseudo-sistemas de archivos,
/// ejecuta una sesión interactiva en el `chroot` y limpia los montajes al finalizar
/// únicamente si no hay servicios del VPS ejecutándose en segundo plano.
pub fn run_shell(rootfs: &Path) -> Result<(), DroidError> {
    verify_rootfs_initialized(rootfs)?;

    // Best-effort: en kernels con SELinux o wakelocks bloqueados (EINVAL/EACCES),
    // el contexto de `su -mm` ya cuenta con permisos suficientes para operar.
    let _ = selinux::ensure_permissive();
    network::setup_android_network_groups()?;
    let _ = power::acquire_wakelock();

    let mount_manager = MountManager::new(rootfs);
    if let Err(mount_err) = mount_manager.mount_all() {
        let _ = mount_manager.unmount_all();
        let _ = power::release_wakelock();
        return Err(mount_err);
    }

    let runner = ChrootRunner::new(rootfs);
    let session_result = runner.run_shell();

    // Preservar montajes y wakelock si el servidor VPS está corriendo en segundo plano
    let ssh_active = SshService::new(rootfs).is_running();
    let tunnel_active = TunnelService::new(rootfs).active_daemon().is_some();

    if !ssh_active && !tunnel_active {
        let unmount_result = mount_manager.unmount_all();
        let _ = power::release_wakelock();
        session_result?;
        unmount_result?;
    } else {
        session_result?;
    }

    Ok(())
}

/// Inicia el nodo VPS en modo headless: adquiere Wakelock, monta el entorno,
/// levanta el servidor Dropbear SSH y arranca el túnel de red si está configurado.
pub fn start_vps(rootfs: &Path) -> Result<VpsStartReport, DroidError> {
    verify_rootfs_initialized(rootfs)?;

    let _ = selinux::ensure_permissive();
    network::setup_android_network_groups()?;
    let _ = power::acquire_wakelock();

    let mount_manager = MountManager::new(rootfs);
    if let Err(mount_err) = mount_manager.mount_all() {
        let _ = mount_manager.unmount_all();
        let _ = power::release_wakelock();
        return Err(mount_err);
    }

    let ssh_service = SshService::new(rootfs);
    let ssh_pid = match ssh_service.start() {
        Ok(pid) => pid,
        Err(err) => {
            let _ = mount_manager.unmount_all();
            let _ = power::release_wakelock();
            return Err(err);
        }
    };

    let tunnel_service = TunnelService::new(rootfs);
    let tunnel_status = tunnel_service.start_if_configured()?;

    Ok(VpsStartReport {
        ssh_pid,
        ssh_port: ssh_service.port(),
        tunnel_status,
    })
}

/// Detiene los demonios del VPS (túneles y SSH), desmonta los sistemas de archivos
/// virtuales del contenedor y libera el Wakelock del kernel Android.
pub fn stop_vps(rootfs: &Path) -> Result<VpsStopReport, DroidError> {
    let tunnel_service = TunnelService::new(rootfs);
    let tunnel_stopped = tunnel_service.stop()?;

    let ssh_service = SshService::new(rootfs);
    let ssh_stopped = ssh_service.stop()?;

    let mount_manager = MountManager::new(rootfs);
    let had_mounts = !mount_manager.active_mounts().is_empty();
    mount_manager.unmount_all()?;

    let _ = power::release_wakelock();

    Ok(VpsStopReport {
        ssh_stopped,
        tunnel_stopped,
        filesystems_unmounted: had_mounts,
    })
}

/// Inspecciona de forma no intrusiva el estado actual del contenedor, montajes y demonios
pub fn inspect_status(rootfs: &Path) -> VpsStatus {
    let rootfs_initialized = rootfs::is_rootfs_initialized(rootfs);

    let mount_manager = MountManager::new(rootfs);
    let active_mounts = mount_manager.active_mounts();

    let ssh_service = SshService::new(rootfs);
    let ssh_pid = ssh_service.running_pid();
    let ssh_port = ssh_service.port();

    let tunnel_service = TunnelService::new(rootfs);
    let tunnel_status = tunnel_service.status();

    VpsStatus {
        rootfs_initialized,
        active_mounts,
        ssh_pid,
        ssh_port,
        tunnel_status,
    }
}

/// Verifica que el directorio `rootfs` contenga la estructura base de Alpine Linux
fn verify_rootfs_initialized(rootfs: &Path) -> Result<(), DroidError> {
    if !rootfs::is_rootfs_initialized(rootfs) {
        return Err(DroidError::Rootfs(format!(
            "The rootfs environment at '{}' is not initialized. Run 'droidvps init' first.",
            rootfs.display()
        )));
    }
    Ok(())
}