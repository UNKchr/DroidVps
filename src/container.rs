pub mod chroot;
pub mod mount;

use crate::android::network::setup_android_network_groups;
use crate::android::power::{acquire_wakelock, release_wakelock};
use crate::android::selinux::ensure_permissive;
use crate::error::DroidError;
use std::path::Path;

// Importamos las estructuras que ya tienes implementadas en los submódulos.
// Se asume el uso de self::mount::MountManager y self::chroot::ChrootRunner.
use self::chroot::ChrootRunner;
use self::mount::MountManager;

/// Orquesta la preparación del kernel, el montaje del entorno, la ejecución del chroot
/// y el desmontaje seguro al finalizar (ciclo de vida completo).
pub fn run_shell(rootfs_path: &Path) -> Result<(), DroidError> {
    // 1. Adecuación del entorno del Kernel de Android
    ensure_permissive()?;
    setup_android_network_groups()?;
    acquire_wakelock()?;

    // 2. Montaje de pseudofilesystems nativos (/dev, /proc, /sys)
    let mounter = MountManager::new(rootfs_path);
    mounter.mount_all()?;

    // 3. Ejecución del entorno
    let runner = ChrootRunner::new(rootfs_path);
    
    // Capturamos el resultado de la shell pero NO retornamos (con `?`) inmediatamente.
    // Esto actúa como un bloque `finally` para asegurar el desmontaje.
    let shell_result = runner.run_shell();

    // 4. Limpieza (Cleanup / Teardown)
    // Es CRÍTICO ejecutar esto incluso si la shell retornó un error (ej. exit code != 0),
    // de lo contrario /data quedaría bloqueado con montajes bind activos.
    let unmount_result = mounter.unmount_all();
    let wakelock_result = release_wakelock();

    // 5. Propagación de errores
    // Devolvemos el primer error encontrado en orden de importancia.
    shell_result?;
    unmount_result?;
    wakelock_result?;

    Ok(())
}