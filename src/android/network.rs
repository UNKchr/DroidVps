use crate::error::DroidError;
use nix::unistd::{getgroups, setgroups, Gid};

/// GID requerido para abrir sockets de red TCP/UDP convencionales en Android.
const AID_INET: u32 = 3003;
/// GID requerido para sockets crudos (ej. ping, trazados de red).
const AID_NET_RAW: u32 = 3004;

/// Inyecta los grupos suplementarios de Android necesarios para eludir
/// el aislamiento de red PARANOID_NETWORK del kernel.
pub fn setup_android_network_groups() -> Result<(), DroidError> {
    // Obtenemos los grupos suplementarios actuales del proceso.
    // El operador `?` convierte automáticamente nix::Error a DroidError::Nix.
    let mut groups = getgroups()?;

    let inet = Gid::from_raw(AID_INET);
    let net_raw = Gid::from_raw(AID_NET_RAW);
    let mut modified = false;

    // Añadimos AID_INET si no está presente
    if !groups.contains(&inet) {
        groups.push(inet);
        modified = true;
    }

    // Añadimos AID_NET_RAW si no está presente
    if !groups.contains(&net_raw) {
        groups.push(net_raw);
        modified = true;
    }

    // Aplicamos la nueva lista de grupos al proceso padre antes del chroot
    if modified {
        setgroups(&groups)?;
    }

    Ok(())
}