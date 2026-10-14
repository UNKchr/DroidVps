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

#[derive(Parser, Debug, PartialEq, Eq)]
#[command(
    name = "droidvps",
    author = "UNKchr",
    version,
    about = "Lightweight orchestrator to turn rooted Android devices into headless Linux VPS servers",
    long_about = None,
    before_help = BANNER
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum Commands {
    /// Download, verify, and deploy the base system (Alpine Linux Minimal aarch64)
    Init {
        /// Target directory path for the rootfs environment on the Android device
        #[arg(short, long, default_value = DEFAULT_ROOTFS_PATH)]
        target: PathBuf,
    },

    /// Mount kernel subsystems and enter an isolated interactive shell inside the container
    Enter {
        /// Target rootfs directory path where chroot will be executed
        #[arg(short, long, default_value = DEFAULT_ROOTFS_PATH)]
        target: PathBuf,
    },

    /// Start the headless VPS node in the background (Wakelock, Dropbear SSH, and tunnels)
    Start {
        /// Target rootfs directory path
        #[arg(short, long, default_value = DEFAULT_ROOTFS_PATH)]
        target: PathBuf,
    },

    /// Stop all VPS background services, unmount filesystems, and release the kernel Wakelock
    Stop {
        /// Target rootfs directory path to stop
        #[arg(short, long, default_value = DEFAULT_ROOTFS_PATH)]
        target: PathBuf,
    },

    /// Display container status, active daemons, and hardware telemetry (battery/SoC/RAM)
    Status {
        /// Target rootfs directory path to inspect
        #[arg(short, long, default_value = DEFAULT_ROOTFS_PATH)]
        target: PathBuf,
    },

    /// Smart assistant to install, authenticate, and manage network tunnels (Tailscale / Cloudflared)
    Tunnel {
        #[command(subcommand)]
        action: TunnelAction,
    },

    /// Install standalone binaries, Termux CLI wrappers (droidvps/dvps), and Android boot autostart
    Setup {
        /// Target rootfs directory path used for boot autostart
        #[arg(short, long, default_value = DEFAULT_ROOTFS_PATH)]
        target: PathBuf,

        /// Enable automatic VPS startup on Android boot (/data/adb/service.d)
        #[arg(long, conflicts_with = "disable_autostart")]
        enable_autostart: bool,

        /// Disable automatic VPS startup on Android boot
        #[arg(long, conflicts_with = "enable_autostart")]
        disable_autostart: bool,
    },
}

#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum TunnelAction {
    /// Automatically install Tailscale, start tailscaled (userspace), and link the account
    Tailscale {
        /// Target rootfs directory path
        #[arg(short, long, default_value = DEFAULT_ROOTFS_PATH)]
        target: PathBuf,

        /// Optional pre-generated authentication key (if omitted, displays interactive login URL)
        #[arg(short, long)]
        auth_key: Option<String>,
    },

    /// Automatically download cloudflared (arm64), save the token, and start the tunnel
    Cloudflared {
        /// Target rootfs directory path
        #[arg(short, long, default_value = DEFAULT_ROOTFS_PATH)]
        target: PathBuf,

        /// Cloudflare Zero Trust tunnel authentication token
        #[arg(long)]
        token: String,
    },

    /// Stop any active network tunnel daemon without shutting down the SSH server
    Stop {
        /// Target rootfs directory path
        #[arg(short, long, default_value = DEFAULT_ROOTFS_PATH)]
        target: PathBuf,
    },
}