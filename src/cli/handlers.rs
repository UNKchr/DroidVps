use std::path::Path;
use anyhow::{bail, Context, Result};
use crate::cli::args::{Commands, TunnelAction};
use crate::container::mount::MountManager;
use crate::services::{TunnelService, TunnelStatus, DEFAULT_ROOT_PASSWORD};
use crate::{android, container, rootfs};

/// Imprime el banner oficial de droidvps con colores ANSI directos (sin crates externos)
pub fn print_banner() {
    const LOGO: &str = "\
██████╗ ██████╗  ██████╗ ██╗██████╗ ██╗   ██╗██████╗ ███████╗
██╔══██╗██╔══██╗██╔═══██╗██║██╔══██╗██║   ██║██╔══██╗██╔════╝
██║  ██║██████╔╝██║   ██║██║██║  ██║██║   ██║██████╔╝███████╗
██║  ██║██╔══██╗██║   ██║██║██║  ██║╚██╗ ██╔╝██╔═══╝ ╚════██║
██████╔╝██║  ██║╚██████╔╝██║██████╔╝ ╚████╔╝ ██║     ███████║
╚═════╝ ╚═╝  ╚═╝ ╚═════╝ ╚═╝╚═════╝   ╚═══╝  ╚═╝     ╚══════╝";

    println!(
        "\x1b[1;36m{}\n\x1b[1;32m  v{} • Developed by UNKchr\x1b[0m\n",
        LOGO,
        env!("CARGO_PKG_VERSION")
    );
}

/// Despachador principal de subcomandos de la interfaz CLI
pub fn handle_command(command: Commands) -> Result<()> {
    match command {
        Commands::Init { target } => {
            print_banner();
            println!(
                "[*] Initializing Alpine Linux aarch64 environment at: {}",
                target.display()
            );

            rootfs::setup_rootfs(&target).with_context(|| {
                format!(
                    "Failed to provision the root filesystem (rootfs) at '{}'",
                    target.display()
                )
            })?;

            println!(
                "\x1b[1;32m[✓] Rootfs successfully provisioned at: {}\x1b[0m",
                target.display()
            );
        }

        Commands::Enter { target } => {
            print_banner();
            println!(
                "[*] Preparing isolation and entering the container at: {}",
                target.display()
            );

            container::run_shell(&target).with_context(|| {
                format!(
                    "Container chroot lifecycle failed at '{}'",
                    target.display()
                )
            })?;

            println!("[*] Interactive session ended.");
        }

        Commands::Start { target } => {
            print_banner();
            println!(
                "[*] Starting VPS node services in the background ({})...",
                target.display()
            );

            let report = container::start_vps(&target).with_context(|| {
                format!(
                    "Failed to start VPS services at '{}'",
                    target.display()
                )
            })?;

            println!(
                "\x1b[1;32m[✓] DroidVPS node is active and running in the background:\x1b[0m"
            );
            println!(
                "  • Dropbear SSH : \x1b[1;32mActive\x1b[0m (PID: {}, TCP port: {})",
                report.ssh_pid, report.ssh_port
            );
            println!(
                "  • Credentials   : user 'root' (default initial password: '{}')",
                DEFAULT_ROOT_PASSWORD
            );

            match report.tunnel_status {
                TunnelStatus::Running { provider, pid } => {
                    println!(
                        "  • Network Tunnel: \x1b[1;32mActive\x1b[0m [{} - PID: {}]",
                        provider.as_str(),
                        pid
                    );
                }
                TunnelStatus::Stopped {
                    configured: Some(provider),
                } => {
                    println!(
                        "  • Network Tunnel: \x1b[1;33mStopped\x1b[0m ({})",
                        provider.as_str()
                    );
                }
                TunnelStatus::Stopped { configured: None } | TunnelStatus::NotConfigured => {
                    println!("  • Network Tunnel: Not configured");
                    println!(
                        "    \x1b[1;36m-> Tip:\x1b[0m Run 'droidvps tunnel tailscale' or 'droidvps tunnel cloudflared --token <TOKEN>' for remote access."
                    );
                }
            }
        }

        Commands::Stop { target } => {
            print_banner();
            println!(
                "[*] Stopping VPS node services and cleaning up mounts ({})...",
                target.display()
            );

            let report = container::stop_vps(&target).with_context(|| {
                format!(
                    "Failed to stop services or unmount the container at '{}'",
                    target.display()
                )
            })?;

            println!(
                "  • Dropbear SSH : {}",
                if report.ssh_stopped {
                    "\x1b[1;32mStopped\x1b[0m"
                } else {
                    "Not running"
                }
            );
            println!(
                "  • Network Tunnel: {}",
                if report.tunnel_stopped {
                    "\x1b[1;32mStopped\x1b[0m"
                } else {
                    "Not running"
                }
            );
            println!(
                "  • Mounts        : {}",
                if report.filesystems_unmounted {
                    "\x1b[1;32mUnmounted (MNT_DETACH)\x1b[0m"
                } else {
                    "No active mounts"
                }
            );
            println!("\x1b[1;32m[✓] Node stopped and wakelock released.\x1b[0m");
        }

        Commands::Status { target } => {
            print_banner();
            let vps = container::inspect_status(&target);

            println!("\x1b[1;36m[ Container and Service Status ]\x1b[0m");
            println!(
                "  Alpine rootfs   : {} ({})",
                target.display(),
                if vps.rootfs_initialized {
                    "\x1b[1;32mInitialized\x1b[0m"
                } else {
                    "\x1b[1;33mNot initialized\x1b[0m"
                }
            );

            if vps.active_mounts.is_empty() {
                println!("  Kernel mounts   : \x1b[1;33mInactive\x1b[0m");
            } else {
                println!(
                    "  Kernel mounts   : \x1b[1;32mActive\x1b[0m ({})",
                    vps.active_mounts.join(", ")
                );
            }

            match vps.ssh_pid {
                Some(pid) => {
                    println!(
                        "  Dropbear SSH    : \x1b[1;32mActive\x1b[0m (PID: {}, Port: {})",
                        pid, vps.ssh_port
                    );
                }
                None => {
                    println!(
                        "  Dropbear SSH    : \x1b[1;33mStopped\x1b[0m (Configured port: {})",
                        vps.ssh_port
                    );
                }
            }

            match vps.tunnel_status {
                TunnelStatus::Running { provider, pid } => {
                    println!(
                        "  Network Tunnel  : \x1b[1;32mActive\x1b[0m [{} - PID: {}]",
                        provider.as_str(),
                        pid
                    );
                }
                TunnelStatus::Stopped {
                    configured: Some(provider),
                } => {
                    println!(
                        "  Network Tunnel  : \x1b[1;33mStopped\x1b[0m ({})",
                        provider.as_str()
                    );
                }
                TunnelStatus::Stopped { configured: None } | TunnelStatus::NotConfigured => {
                    println!("  Network Tunnel  : Not configured");
                }
            }

            println!("\n\x1b[1;36m[ Hardware and Kernel Telemetry ]\x1b[0m");
            let telemetry = android::collect_telemetry()
                .context("Failed to collect hardware telemetry from sysfs/procfs")?;

            if let Some(uptime) = telemetry.uptime_seconds {
                let hours = uptime / 3600;
                let minutes = (uptime % 3600) / 60;
                println!("  Uptime Kernel   : {}h {}m ({}s)", hours, minutes, uptime);
            }

            if let Some(mem) = telemetry.memory {
                println!(
                    "  RAM             : {} MB / {} MB ({}% used, {} MB free)",
                    mem.used_mb, mem.total_mb, mem.usage_percent, mem.available_mb
                );
            }

            match telemetry.battery {
                Some(bat) => {
                    let temp_info = bat
                        .temperature_c
                        .map(|t| format!(", {:.1} °C", t))
                        .unwrap_or_default();
                    let volt_info = bat
                        .voltage_mv
                        .map(|v| format!(", {} mV", v))
                        .unwrap_or_default();
                    println!(
                        "  Battery ({})    : {}% [{}]{}{}",
                        bat.supply_name, bat.capacity_percent, bat.status, temp_info, volt_info
                    );
                }
                None => {
                    println!("  Battery         : Not detected (no physical power_supply)");
                }
            }

            match telemetry.max_soc_temp_c {
                Some(max_temp) => {
                    println!(
                        "  SoC temperature : {:.1} °C max ({} active thermal zones)",
                        max_temp,
                        telemetry.thermal_zones.len()
                    );
                }
                None => {
                    println!("  SoC temperature : No thermal sensors reported in sysfs");
                }
            }
        }

        Commands::Tunnel { action } => {
            print_banner();
            handle_tunnel_action(action)?;
        }
    }

    Ok(())
}

/// Gestiona las operaciones del asistente inteligente de túneles (`droidvps tunnel ...`)
fn handle_tunnel_action(action: TunnelAction) -> Result<()> {
    match action {
        TunnelAction::Tailscale { target, auth_key } => {
            println!(
                "[*] Preparing the automatic Tailscale assistant at: {}",
                target.display()
            );
            prepare_runtime_environment(&target)?;

            let tunnel = TunnelService::new(&target);
            let pid = tunnel
                .setup_and_authenticate_tailscale(auth_key.as_deref())
                .context("Tailscale provisioning or authentication failed")?;

            println!(
                "\x1b[1;32m[✓] Tailscale active and authenticated in the background (PID: {}).\x1b[0m",
                pid
            );
        }

        TunnelAction::Cloudflared { target, token } => {
            println!(
                "[*] Preparing the automatic Cloudflare Tunnel assistant at: {}",
                target.display()
            );
            prepare_runtime_environment(&target)?;

            let tunnel = TunnelService::new(&target);
            let pid = tunnel
                .setup_and_start_cloudflared(&token)
                .context("cloudflared download, configuration, or startup failed")?;

            println!(
                "\x1b[1;32m[✓] Cloudflare Tunnel (cloudflared) active in the background (PID: {}).\x1b[0m",
                pid
            );
            println!("    -> Event log available at <rootfs>/var/log/cloudflared.log");
        }

        TunnelAction::Stop { target } => {
            println!(
                "[*] Stopping tunnel services at: {}...",
                target.display()
            );
            let tunnel = TunnelService::new(&target);
            let stopped = tunnel
                .stop()
                .context("Failed to stop tunnel daemons")?;

            if stopped {
                println!("\x1b[1;32m[✓] Tunnel daemon stopped successfully.\x1b[0m");
            } else {
                println!("[*] No running tunnel daemons were found.");
            }
        }
    }

    Ok(())
}

/// Garantiza que el rootfs esté inicializado y que los montajes/permisos del kernel Android estén activos
fn prepare_runtime_environment(target: &Path) -> Result<()> {
    if !target.join("bin/sh").exists() || !target.join("etc/alpine-release").exists() {
        bail!(
            "The rootfs environment at '{}' is not initialized. Run 'droidvps init' first.",
            target.display()
        );
    }

    rootfs::configure_android_groups(target)
        .context("Could not ensure Android network groups in /etc/group")?;
    android::selinux::ensure_permissive()
        .context("Could not verify/configure SELinux permissive mode")?;
    android::network::setup_android_network_groups()
        .context("Could not inject Android network GIDs (3003/3004)")?;
    android::power::acquire_wakelock()
        .context("Could not acquire the kernel wakelock")?;

    let mount_manager = MountManager::new(target);
    mount_manager
        .mount_all()
        .context("Could not mount pseudo-filesystems (/dev, /proc, /sys)")?;

    Ok(())
}