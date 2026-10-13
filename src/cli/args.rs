use std::path::PathBuf;
use clap::{Parser, Subcommand};
use crate::rootfs::DEFAULT_ROOTFS_PATH;

/// Official ASCII banner with ANSI sequences for the Clap help header
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
    about = "Lightweight orchestrator for converting rooted Android devices into headless Linux servers (VPS)",
    long_about = None,
    before_help = BANNER
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Downloads, verifies, and deploys the base system (Alpine Linux Minimal aarch64)
    Init {
        /// Destination path for the rootfs environment on the Android device
        #[arg(short, long, default_value = DEFAULT_ROOTFS_PATH)]
        target: PathBuf,
    },

    /// Mounts kernel subsystems and starts an isolated interactive shell in the container
    Enter {
        /// Rootfs path where the chroot will run
        #[arg(short, long, default_value = DEFAULT_ROOTFS_PATH)]
        target: PathBuf,
    },

    /// Starts the VPS server in the background (wakelock, Dropbear SSH, and tunnels)
    Start {
        /// Active rootfs path
        #[arg(short, long, default_value = DEFAULT_ROOTFS_PATH)]
        target: PathBuf,
    },

    /// Stops VPS services, unmounts filesystems, and releases the wakelock
    Stop {
        /// Rootfs path to stop
        #[arg(short, long, default_value = DEFAULT_ROOTFS_PATH)]
        target: PathBuf,
    },

    /// Shows container status, active services, and hardware telemetry (battery/CPU)
    Status {
        /// Rootfs path to inspect
        #[arg(short, long, default_value = DEFAULT_ROOTFS_PATH)]
        target: PathBuf,
    },

    /// Intelligent assistant for installing, authenticating, and managing network tunnels (Tailscale / Cloudflared)
    Tunnel {
        #[command(subcommand)]
        action: TunnelAction,
    },
}

#[derive(Subcommand, Debug)]
pub enum TunnelAction {
    /// Automatically installs Tailscale, starts tailscaled (userspace), and links the account
    Tailscale {
        /// Active rootfs path
        #[arg(short, long, default_value = DEFAULT_ROOTFS_PATH)]
        target: PathBuf,

        /// Optional pre-generated authentication key (if omitted, shows the interactive link)
        #[arg(short, long)]
        auth_key: Option<String>,
    },

    /// Automatically downloads cloudflared (arm64), stores the token, and starts the tunnel
    Cloudflared {
        /// Active rootfs path
        #[arg(short, long, default_value = DEFAULT_ROOTFS_PATH)]
        target: PathBuf,

        /// Cloudflare Zero Trust tunnel authentication token
        #[arg(long)]
        token: String,
    },

    /// Stops any active tunnel daemon without shutting down the SSH server
    Stop {
        /// Active rootfs path
        #[arg(short, long, default_value = DEFAULT_ROOTFS_PATH)]
        target: PathBuf,
    },
}