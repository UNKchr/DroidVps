use std::path::Path;
use anyhow::{bail, Context, Result};
use crate::android::env::TERMUX_BIN_DIR;
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
            android::ensure_root_privileges()
                .context("Cannot initialize rootfs without root privileges")?;
            android::network::setup_android_network_groups()
                .context("Failed to initialize Android network groups and DNS resolver")?;

            println!(
                "[*] Initializing Alpine Linux aarch64 environment at: {}",
                target.display()
            );

            rootfs::setup_rootfs(&target).with_context(|| {
                format!(
                    "Failed to provision root filesystem at '{}'",
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
            android::ensure_root_privileges()
                .context("Cannot enter container without root privileges")?;

            println!(
                "[*] Preparing isolation and entering container at: {}",
                target.display()
            );

            container::run_shell(&target).with_context(|| {
                format!(
                    "Error during chroot container lifecycle at '{}'",
                    target.display()
                )
            })?;

            println!("[*] Interactive session ended.");
        }

        Commands::Start { target } => {
            print_banner();
            android::ensure_root_privileges()
                .context("Cannot start VPS services without root privileges")?;

            println!(
                "[*] Starting headless VPS node services in the background ({})...",
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
                "  • Dropbear SSH : \x1b[1;32mRunning\x1b[0m (PID: {}, TCP Port: {})",
                report.ssh_pid, report.ssh_port
            );
            println!(
                "  • Credentials  : user 'root' (initial default password: '{}')",
                DEFAULT_ROOT_PASSWORD
            );

            match report.tunnel_status {
                TunnelStatus::Running { provider, pid } => {
                    println!(
                        "  • Net Tunnel   : \x1b[1;32mRunning\x1b[0m [{} - PID: {}]",
                        provider.as_str(),
                        pid
                    );
                }
                TunnelStatus::Stopped {
                    configured: Some(provider),
                } => {
                    println!(
                        "  • Net Tunnel   : \x1b[1;33mStopped\x1b[0m ({})",
                        provider.as_str()
                    );
                }
                TunnelStatus::Stopped { configured: None } | TunnelStatus::NotConfigured => {
                    println!("  • Net Tunnel   : Not configured");
                    println!(
                        "    \x1b[1;36m-> Tip:\x1b[0m Run 'droidvps tunnel tailscale' or 'droidvps tunnel cloudflared --token <TOKEN>' for remote access."
                    );
                }
            }
        }

        Commands::Stop { target } => {
            print_banner();
            android::ensure_root_privileges()
                .context("Cannot stop VPS services without root privileges")?;

            println!(
                "[*] Stopping VPS node services and cleaning up mounts ({})...",
                target.display()
            );

            let report = container::stop_vps(&target).with_context(|| {
                format!(
                    "Failed to stop services or unmount container at '{}'",
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
                "  • Net Tunnel   : {}",
                if report.tunnel_stopped {
                    "\x1b[1;32mStopped\x1b[0m"
                } else {
                    "Not running"
                }
            );
            println!(
                "  • Filesystems  : {}",
                if report.filesystems_unmounted {
                    "\x1b[1;32mUnmounted (MNT_DETACH)\x1b[0m"
                } else {
                    "No active mounts"
                }
            );
            println!("\x1b[1;32m[✓] Node stopped and kernel Wakelock released.\x1b[0m");
        }

        Commands::Status { target } => {
            print_banner();
            let vps = container::inspect_status(&target);
            let autostart = android::is_boot_autostart_enabled(None);

            println!("\x1b[1;36m[ Container & Services Status ]\x1b[0m");
            println!(
                "  Alpine Rootfs   : {} ({})",
                target.display(),
                if vps.rootfs_initialized {
                    "\x1b[1;32mInitialized\x1b[0m"
                } else {
                    "\x1b[1;33mNot initialized\x1b[0m"
                }
            );

            if vps.active_mounts.is_empty() {
                println!("  Kernel Mounts   : \x1b[1;33mInactive\x1b[0m");
            } else {
                println!(
                    "  Kernel Mounts   : \x1b[1;32mActive\x1b[0m ({})",
                    vps.active_mounts.join(", ")
                );
            }

            match vps.ssh_pid {
                Some(pid) => {
                    println!(
                        "  Dropbear SSH    : \x1b[1;32mRunning\x1b[0m (PID: {}, Port: {})",
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
                        "  Network Tunnel  : \x1b[1;32mRunning\x1b[0m [{} - PID: {}]",
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

            println!(
                "  Boot Autostart  : {}",
                if autostart {
                    "\x1b[1;32mEnabled\x1b[0m (/data/adb/service.d)"
                } else {
                    "\x1b[1;33mDisabled\x1b[0m"
                }
            );

            println!("\n\x1b[1;36m[ Hardware & Kernel Telemetry ]\x1b[0m");
            let telemetry = android::collect_telemetry()
                .context("Failed to collect hardware telemetry from sysfs/procfs")?;

            if let Some(uptime) = telemetry.uptime_seconds {
                let hours = uptime / 3600;
                let minutes = (uptime % 3600) / 60;
                println!("  Kernel Uptime   : {}h {}m ({}s)", hours, minutes, uptime);
            }

            if let Some(mem) = telemetry.memory {
                println!(
                    "  System Memory   : {} MB / {} MB ({}% used, {} MB available)",
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
                        "  Battery ({}) : {}% [{}]{}{}",
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
                        "  SoC Temperature : {:.1} °C max ({} active thermal zones)",
                        max_temp,
                        telemetry.thermal_zones.len()
                    );
                }
                None => {
                    println!("  SoC Temperature : No thermal sensors reported in sysfs");
                }
            }
        }

        Commands::Tunnel { action } => {
            print_banner();
            android::ensure_root_privileges()
                .context("Cannot manage network tunnels without root privileges")?;
            handle_tunnel_action(action)?;
        }

        Commands::Setup {
            target,
            enable_autostart,
            disable_autostart,
        } => {
            print_banner();
            android::ensure_root_privileges()
                .context("Cannot configure standalone environment without root privileges")?;

            println!("[*] Configuring standalone DroidVPS environment...");
            let installed_bin = android::setup_standalone_environment(None)
                .context("Failed to install standalone binary and wrappers")?;

            println!(
                "  • System Binary : \x1b[1;32mInstalled\x1b[0m ({})",
                installed_bin.display()
            );

            if Path::new(TERMUX_BIN_DIR).exists() {
                println!(
                    "  • Termux Bridge : \x1b[1;32mInstalled\x1b[0m ('droidvps' and 'dvps' available in Termux)"
                );
            } else {
                println!("  • Termux Bridge : Skipped (Termux directory not found)");
            }

            if enable_autostart {
                let script_path = android::configure_boot_autostart(&target, None)
                    .context("Failed to configure boot autostart script in /data/adb/service.d")?;
                println!(
                    "  • Boot Service  : \x1b[1;32mEnabled\x1b[0m ({})",
                    script_path.display()
                );
            } else if disable_autostart {
                let removed = android::remove_boot_autostart(None)
                    .context("Failed to remove boot autostart script")?;
                println!(
                    "  • Boot Service  : {}",
                    if removed {
                        "\x1b[1;33mDisabled & Removed\x1b[0m"
                    } else {
                        "Already disabled"
                    }
                );
            }

            println!("\x1b[1;32m[✓] Standalone setup completed successfully.\x1b[0m");
        }
    }

    Ok(())
}

/// Gestiona las operaciones del asistente inteligente de túneles (`droidvps tunnel ...`)
fn handle_tunnel_action(action: TunnelAction) -> Result<()> {
    match action {
        TunnelAction::Tailscale { target, auth_key } => {
            println!(
                "[*] Preparing automated Tailscale assistant at: {}",
                target.display()
            );
            prepare_runtime_environment(&target)?;

            let tunnel = TunnelService::new(&target);
            let pid = tunnel
                .setup_and_authenticate_tailscale(auth_key.as_deref())
                .context("Failed during Tailscale provisioning or authentication")?;

            println!(
                "\x1b[1;32m[✓] Tailscale is active and authenticated in the background (PID: {}).\x1b[0m",
                pid
            );
        }

        TunnelAction::Cloudflared { target, token } => {
            println!(
                "[*] Preparing automated Cloudflare Tunnel assistant at: {}",
                target.display()
            );
            prepare_runtime_environment(&target)?;

            let tunnel = TunnelService::new(&target);
            let pid = tunnel
                .setup_and_start_cloudflared(&token)
                .context("Failed during cloudflared download, configuration, or startup")?;

            println!(
                "\x1b[1;32m[✓] Cloudflare Tunnel (cloudflared) is active in the background (PID: {}).\x1b[0m",
                pid
            );
            println!("    -> Event log available at <rootfs>/var/log/cloudflared.log");
        }

        TunnelAction::Stop { target } => {
            println!(
                "[*] Stopping network tunnel services at: {}...",
                target.display()
            );
            let tunnel = TunnelService::new(&target);
            let stopped = tunnel
                .stop()
                .context("Failed while attempting to stop tunnel daemons")?;

            if stopped {
                println!("\x1b[1;32m[✓] Network tunnel daemon stopped successfully.\x1b[0m");
            } else {
                println!("[*] No active network tunnel daemons found.");
            }
        }
    }

    Ok(())
}

/// Garantiza que el rootfs esté inicializado y que los montajes/permisos del kernel Android estén activos
fn prepare_runtime_environment(target: &Path) -> Result<()> {
    if !rootfs::is_rootfs_initialized(target) {
        bail!(
            "The rootfs environment at '{}' is not initialized. Run 'droidvps init' first.",
            target.display()
        );
    }

    rootfs::configure_android_groups(target)
        .context("Failed to ensure Android network groups in /etc/group")?;

    // Best-effort en kernels donde SELinux o wakelocks en sysfs devuelven EINVAL/EACCES
    let _ = android::selinux::ensure_permissive();
    android::network::setup_android_network_groups()
        .context("Failed to inject Android network GIDs (3003/3004)")?;
    let _ = android::power::acquire_wakelock();

    let mount_manager = MountManager::new(target);
    mount_manager
        .mount_all()
        .context("Failed to mount pseudo-filesystems (/dev, /proc, /sys)")?;

    Ok(())
}