use crate::error::DroidError;
use std::fs;
use std::path::Path;

const WAKE_LOCK_PATH: &str = "/sys/power/wake_lock";
const WAKE_UNLOCK_PATH: &str = "/sys/power/wake_unlock";
const WAKELOCK_TAG: &str = "droidvps_core_lock";

/// Adquiere un bloqueo de suspensión del kernel para evitar que la CPU 
/// entre en estado de Deep Sleep mientras la pantalla del dispositivo está apagada.
pub fn acquire_wakelock() -> Result<(), DroidError> {
    let path = Path::new(WAKE_LOCK_PATH);
    
    if !path.exists() {
        return Err(DroidError::Android(
            "The /sys/power/wake_lock node does not exist in this device's kernel.".to_string(),
        ));
    }

    // Usamos el operador `?` para delegar automáticamente cualquier std::io::Error 
    // a DroidError::Io (ej. EACCES, ENOSPC).
    fs::write(path, WAKELOCK_TAG)?;

    Ok(())
}

/// Libera el bloqueo de suspensión devolviendo el control al gestor de 
/// energía nativo de Android.
pub fn release_wakelock() -> Result<(), DroidError> {
    let path = Path::new(WAKE_UNLOCK_PATH);
    
    // Si la ruta existe, procedemos a liberar. Si no existe, no es un error crítico.
    if path.exists() {
        fs::write(path, WAKELOCK_TAG)?;
    }
    
    Ok(())
}