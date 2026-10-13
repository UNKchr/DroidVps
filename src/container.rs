pub mod chroot;
pub mod mount;

use std::path::Path;
use crate::android::network::setup_android_network_groups;
use crate::android::power::{acquire_wakelock, release_wakelock};
use crate::android::selinux::ensure_permissive;
use crate::container::chroot::ChrootRunner;
use crate::container::mount::MountManager;
use crate::error::DroidError;
use crate::services::{SshService, TunnelService, TunnelStatus};

/// Resumen devuelto al iniciar los servicios del nodo VPS en segundo plano
#[derive(Debug, Clone)]
pub struct VpsStartReport {
    pub ssh_pid: u32,
    pub ssh_port: u16,
    pub tunnel_status: TunnelStatus,
}

/// Resumen devuelto al detener el nodo VPS y liberar recursos del kernel
#[derive(Debug, Clone)]
pub struct VpsStopReport {
    pub ssh_stopped: bool,
    pub tunnel_stopped: bool,
    pub filesystems_unmounted: bool,
}

/// Estado operativo del contenedor y sus demonios asociados
#[derive(Debug, Clone)]
pub struct VpsStatusReport {
    pub rootfs_initialized: bool,
    pub active_mounts: Vec<String>,
    pub ssh_pid: Option<u32>,
    pub ssh_port: u16,
    pub tunnel_status: TunnelStatus,
}

/// Ejecuta una sesión de shell interactiva aislada dentro del contenedor.
/// Si el VPS tiene demonios activos en segundo plano, preserva los montajes y el Wakelock al salir.
pub fn run_shell(rootfs: &Path) -> Result<(), DroidError> {
    verify_rootfs_initialized(rootfs)?;

    let mount_manager = MountManager::new(rootfs);
    let had_active_services = has_running_daemons(rootfs);

    ensure_permissive()?;
    setup_android_network_groups()?;
    acquire_wakelock()?;

    mount_manager.mount_all()?;

    let runner = ChrootRunner::new(rootfs);
    let shell_result = runner.run_shell();

    // Solo desmontar y liberar el wakelock si no hay demonios VPS corriendo en segundo plano
    let services_still_running = has_running_daemons(rootfs);
    if !had_active_services && !services_still_running {
        let unmount_result = mount_manager.unmount_all();
        let wakelock_result = release_wakelock();

        shell_result?;
        unmount_result?;
        wakelock_result?;
    } else {
        shell_result?;
    }

    Ok(())
}

/// Inicia el nodo VPS en segundo plano (Wakelock, montajes, Dropbear SSH y túneles configurados)
pub fn start_vps(rootfs: &Path) -> Result<VpsStartReport, DroidError> {
    verify_rootfs_initialized(rootfs)?;

    ensure_permissive()?;
    setup_android_network_groups()?;
    acquire_wakelock()?;

    let mount_manager = MountManager::new(rootfs);
    mount_manager.mount_all()?;

    let ssh_service = SshService::new(rootfs);
    let ssh_pid = ssh_service.start()?;

    let tunnel_service = TunnelService::new(rootfs);
    let tunnel_status = tunnel_service.start_if_configured()?;

    Ok(VpsStartReport {
        ssh_pid,
        ssh_port: ssh_service.port(),
        tunnel_status,
    })
}

/// Detiene todos los servicios en segundo plano, desmonta el contenedor y libera el Wakelock
pub fn stop_vps(rootfs: &Path) -> Result<VpsStopReport, DroidError> {
    let tunnel_service = TunnelService::new(rootfs);
    let tunnel_stopped = tunnel_service.stop()?;

    let ssh_service = SshService::new(rootfs);
    let ssh_stopped = ssh_service.stop()?;

    let mount_manager = MountManager::new(rootfs);
    let had_mounts = mount_manager.is_mounted();
    mount_manager.unmount_all()?;

    let _ = release_wakelock();

    Ok(VpsStopReport {
        ssh_stopped,
        tunnel_stopped,
        filesystems_unmounted: had_mounts,
    })
}

/// Inspecciona de forma no intrusiva el estado de montajes y servicios del contenedor
pub fn inspect_status(rootfs: &Path) -> VpsStatusReport {
    let rootfs_initialized = is_rootfs_initialized(rootfs);
    let mount_manager = MountManager::new(rootfs);
    let ssh_service = SshService::new(rootfs);
    let tunnel_service = TunnelService::new(rootfs);

    VpsStatusReport {
        rootfs_initialized,
        active_mounts: mount_manager.active_mounts(),
        ssh_pid: ssh_service.running_pid(),
        ssh_port: ssh_service.port(),
        tunnel_status: tunnel_service.status(),
    }
}

/// Comprueba si el directorio rootfs contiene una estructura base de Alpine Linux válida
fn is_rootfs_initialized(rootfs: &Path) -> bool {
    rootfs.join("bin/sh").exists() && rootfs.join("etc/alpine-release").exists()
}

/// Valida que el rootfs esté inicializado antes de intentar montar o hacer chroot
fn verify_rootfs_initialized(rootfs: &Path) -> Result<(), DroidError> {
    if !is_rootfs_initialized(rootfs) {
        return Err(DroidError::Rootfs(format!(
            "The rootfs environment at '{}' is not initialized. Run 'droidvps init' first.",
            rootfs.display()
        )));
    }
    Ok(())
}

/// Devuelve `true` si Dropbear SSH o algún demonio de túnel se encuentra activo
fn has_running_daemons(rootfs: &Path) -> bool {
    let ssh_running = SshService::new(rootfs).is_running();
    let tunnel_running = TunnelService::new(rootfs).active_daemon().is_some();
    ssh_running || tunnel_running
}