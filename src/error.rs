use thiserror::Error;

/// Enum centralizado para el manejo de errores de droidvps.
#[derive(Error, Debug)]
pub enum DroidError {
    #[error("Input/Output Error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Nix system call error: {0}")]
    Nix(#[from] nix::Error),

    #[error("Android Kernel interaction error: {0}")]
    Android(String),

    #[error("Rootfs handling or provisioning error: {0}")]
    Rootfs(String),
}