use std::path::PathBuf;
use clap::{Parser, Subcommand};
use crate::rootfs::DEFAULT_ROOTFS_PATH;

/// Banner ASCII oficial con secuencias ANSI para la cabecera de ayuda de Clap
pub const BANNER: &str = concat!(
    "\x1b[1;36m",
    "██████╗ ██████╗  ██████╗ ██╗██████╗ ██╗   ██╗██████╗ ███████╗\n",
    "██╔══██╗██╔══██╗██╔═══██╗██║██╔══██╗██║   ██║██╔══██╗██╔════╝\n",
    "██║  ██║██████╔╝██║   ██║██║██║  ██║██║   ██║██████╔╝███████╗\n",
    "██║  ██║██╔══██╗██║   ██║██║██║  ██║╚██╗ ██╔╝██╔═══╝ ╚════██║\n",
    "██████╔╝██║  ██║╚██████╔╝██║██████╔╝ ╚████╔╝ ██║     ███████║\n",
    "╚═════╝ ╚═╝  ╚═╝ ╚═════╝ ╚═╝╚═════╝   ╚═══╝  ╚═╝     ╚══════╝\n",
    "\x1b[1;32m  v",
    env!("CARGO_PKG_VERSION"),
    " • Developed by UNKchr\x1b[0m"
);

#[derive(Parser, Debug)]
#[command(
    name = "droidvps",
    author = "UNKchr",
    version,
    about = "Orquestador ligero para convertir dispositivos Android root en servidores Linux headless (VPS)",
    long_about = None,
    before_help = BANNER
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Descarga, verifica y despliega el sistema base (Alpine Linux Minimal aarch64)
    Init {
        /// Ruta de destino para el entorno rootfs en el dispositivo Android
        #[arg(short, long, default_value = DEFAULT_ROOTFS_PATH)]
        target: PathBuf,
    },

    /// Monta subsistemas del kernel e inicia una shell interactiva aislada en el contenedor
    Enter {
        /// Ruta del entorno rootfs donde se ejecutará el chroot
        #[arg(short, long, default_value = DEFAULT_ROOTFS_PATH)]
        target: PathBuf,
    },

    /// Inicia el servidor VPS en segundo plano (Wakelock, Dropbear SSH y túneles)
    Start {
        /// Ruta del entorno rootfs activo
        #[arg(short, long, default_value = DEFAULT_ROOTFS_PATH)]
        target: PathBuf,
    },

    /// Detiene los servicios del VPS, desmonta los sistemas de archivos y libera el Wakelock
    Stop {
        /// Ruta del entorno rootfs a detener
        #[arg(short, long, default_value = DEFAULT_ROOTFS_PATH)]
        target: PathBuf,
    },

    /// Muestra el estado del contenedor, servicios activos y telemetría de hardware (batería/CPU)
    Status {
        /// Ruta del entorno rootfs a inspeccionar
        #[arg(short, long, default_value = DEFAULT_ROOTFS_PATH)]
        target: PathBuf,
    },
}