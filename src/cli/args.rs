use clap::{Parser, Subcommand};

/// Generamos el banner en tiempo de compilación integrando secuencias ANSI
/// y extrayendo la versión directamente del Cargo.toml.
pub const BANNER: &str = concat!(
    "\x1b[1;36m",
    "██████╗ ██████╗  ██████╗ ██╗██████╗ ██╗   ██╗██████╗ ███████╗\n",
    "██╔══██╗██╔══██╗██╔═══██╗██║██╔══██╗██║   ██║██╔══██╗██╔════╝\n",
    "██║  ██║██████╔╝██║   ██║██║██║  ██║██║   ██║██████╔╝███████╗\n",
    "██║  ██║██╔══██╗██║   ██║██║██║  ██║╚██╗ ██╔╝██╔═══╝ ╚════██║\n",
    "██████╔╝██║  ██║╚██████╔╝██║██████╔╝ ╚████╔╝ ██║     ███████║\n",
    "╚═════╝ ╚═╝  ╚═╝ ╚═════╝ ╚═╝╚═════╝   ╚═══╝  ╚═╝     ╚══════╝\n",
    "\x1b[0m",
    "  v", env!("CARGO_PKG_VERSION"), " • Developed by \x1b[1;32mUNKchr\x1b[0m\n"
);

#[derive(Parser, Debug)]
#[command(name = "droidvps")]
#[command(about = "Orquestador de contenedores Linux para dispositivos Android", long_about = None)]
#[command(before_help = BANNER)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Descarga y aprovisiona el rootfs de Alpine Linux
    Init {
        #[arg(long, default_value = crate::rootfs::DEFAULT_ROOTFS_PATH)]
        path: String,
    },
    
    /// Entra a una shell interactiva en el contenedor
    Enter {
        #[arg(long, default_value = crate::rootfs::DEFAULT_ROOTFS_PATH)]
        path: String,
    },
    
    /// Inicializa los servicios del contenedor (ej. SSH)
    Start {
        #[arg(long, help = "Ejecuta los servicios en segundo plano")]
        daemon: bool,
    },
    
    /// Detiene los servicios y desmonta de forma segura
    Stop,
    
    /// Reporta telemetría e inspecciona el estado del entorno
    Status,
}