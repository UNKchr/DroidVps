use crate::error::DroidError;
use std::fs;
use std::path::Path;

const SELINUX_ENFORCE_PATH: &str = "/sys/fs/selinux/enforce";

/// Verifica si SELinux está en modo Enforcing.
/// Retorna `true` si está en Enforcing (1), `false` si está en Permissive (0).
pub fn is_enforcing() -> Result<bool, DroidError> {
    let path = Path::new(SELINUX_ENFORCE_PATH);

    // Si el nodo de SELinux no existe, asumimos que no hay enforcement a nivel kernel
    // (kernel muy antiguo o compilado sin soporte de SELinux).
    if !path.exists() {
        return Ok(false);
    }

    let status = fs::read_to_string(path)?;
    let status = status.trim();

    match status {
        "1" => Ok(true),
        "0" => Ok(false),
        _ => Err(DroidError::Android(format!(
            "Unknown SELinux state in sysfs: {}", status
        ))),
    }
}

/// Cambia el estado de SELinux a Permissive escribiendo '0' en el sysfs.
pub fn set_permissive() -> Result<(), DroidError> {
    let path = Path::new(SELINUX_ENFORCE_PATH);

    if !path.exists() {
        return Err(DroidError::Android(
            "Permissive mode cannot be enabled: the SELinux interface does not exist.".to_string(),
        ));
    }

    // Solicitamos al kernel el cambio de contexto
    fs::write(path, "0")?;

    // Verificamos si el kernel aplicó el cambio realmente (algunos kernels OEM 
    // bloquean este nodo incluso siendo root, ej. Samsung Knox).
    let current_status = fs::read_to_string(path)?;
    if current_status.trim() != "0" {
        return Err(DroidError::Android(
            "The kernel rejected the change to SELinux Permissive (possible hard-enforcing OEM).".to_string(),
        ));
    }

    Ok(())
}

/// Helper para el pipeline principal: asegura el modo Permissive solo si es necesario.
pub fn ensure_permissive() -> Result<(), DroidError> {
    if is_enforcing()? {
        set_permissive()?;
    }
    Ok(())
}