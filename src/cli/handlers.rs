use anyhow::{Context, Result};
use crate::cli::args::Commands;
use crate::services::{TunnelStatus, DEFAULT_ROOT_PASSWORD};
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
                "[*] Inicializando entorno Alpine Linux aarch64 en: {}",
                target.display()
            );

            rootfs::setup_rootfs(&target).with_context(|| {
                format!(
                    "Fallo al aprovisionar el sistema de archivos raíz (rootfs) en '{}'",
                    target.display()
                )
            })?;

            println!(
                "\x1b[1;32m[✓] Rootfs aprovisionado exitosamente en: {}\x1b[0m",
                target.display()
            );
        }

        Commands::Enter { target } => {
            print_banner();
            println!(
                "[*] Preparando aislamiento y entrando al contenedor en: {}",
                target.display()
            );

            container::run_shell(&target).with_context(|| {
                format!(
                    "Fallo durante el ciclo de vida del contenedor chroot en '{}'",
                    target.display()
                )
            })?;

            println!("[*] Sesión interactiva finalizada.");
        }

        Commands::Start { target } => {
            print_banner();
            println!(
                "[*] Iniciando servicios del nodo VPS en segundo plano ({})...",
                target.display()
            );

            let report = container::start_vps(&target).with_context(|| {
                format!(
                    "Fallo al iniciar los servicios del VPS en '{}'",
                    target.display()
                )
            })?;

            println!(
                "\x1b[1;32m[✓] Nodo DroidVPS activo y operando en segundo plano:\x1b[0m"
            );
            println!(
                "  • Dropbear SSH : \x1b[1;32mActivo\x1b[0m (PID: {}, Puerto TCP: {})",
                report.ssh_pid, report.ssh_port
            );
            println!(
                "  • Credenciales : usuario 'root' (contraseña inicial por defecto: '{}')",
                DEFAULT_ROOT_PASSWORD
            );

            match report.tunnel_status {
                TunnelStatus::Running { provider, pid } => {
                    println!(
                        "  • Túnel de Red : \x1b[1;32mActivo\x1b[0m [{} - PID: {}]",
                        provider.as_str(),
                        pid
                    );
                }
                TunnelStatus::Stopped { configured: Some(provider) } => {
                    println!(
                        "  • Túnel de Red : \x1b[1;33mDetenido\x1b[0m ({})",
                        provider.as_str()
                    );
                }
                TunnelStatus::Stopped { configured: None } | TunnelStatus::NotConfigured => {
                    println!(
                        "  • Túnel de Red : No configurado (opcional en /etc/droidvps/)"
                    );
                }
            }
        }

        Commands::Stop { target } => {
            print_banner();
            println!(
                "[*] Deteniendo servicios del nodo VPS y limpiando montajes ({})...",
                target.display()
            );

            let report = container::stop_vps(&target).with_context(|| {
                format!(
                    "Fallo al detener los servicios o desmontar el contenedor en '{}'",
                    target.display()
                )
            })?;

            println!(
                "  • Dropbear SSH : {}",
                if report.ssh_stopped {
                    "\x1b[1;32mDetenido\x1b[0m"
                } else {
                    "No estaba en ejecución"
                }
            );
            println!(
                "  • Túnel de Red : {}",
                if report.tunnel_stopped {
                    "\x1b[1;32mDetenido\x1b[0m"
                } else {
                    "No estaba en ejecución"
                }
            );
            println!(
                "  • Montajes     : {}",
                if report.filesystems_unmounted {
                    "\x1b[1;32mDesmontados (MNT_DETACH)\x1b[0m"
                } else {
                    "Sin montajes activos"
                }
            );
            println!("\x1b[1;32m[✓] Nodo detenido y Wakelock liberado.\x1b[0m");
        }

        Commands::Status { target } => {
            print_banner();
            let vps = container::inspect_status(&target);

            println!("\x1b[1;36m[ Estado del Contenedor y Servicios ]\x1b[0m");
            println!(
                "  Rootfs Alpine   : {} ({})",
                target.display(),
                if vps.rootfs_initialized {
                    "\x1b[1;32mInicializado\x1b[0m"
                } else {
                    "\x1b[1;33mNo inicializado\x1b[0m"
                }
            );

            if vps.active_mounts.is_empty() {
                println!("  Montajes Kernel : \x1b[1;33mInactivos\x1b[0m");
            } else {
                println!(
                    "  Montajes Kernel : \x1b[1;32mActivos\x1b[0m ({})",
                    vps.active_mounts.join(", ")
                );
            }

            match vps.ssh_pid {
                Some(pid) => {
                    println!(
                        "  Dropbear SSH    : \x1b[1;32mActivo\x1b[0m (PID: {}, Puerto: {})",
                        pid, vps.ssh_port
                    );
                }
                None => {
                    println!(
                        "  Dropbear SSH    : \x1b[1;33mDetenido\x1b[0m (Puerto configurado: {})",
                        vps.ssh_port
                    );
                }
            }

            match vps.tunnel_status {
                TunnelStatus::Running { provider, pid } => {
                    println!(
                        "  Túnel de Red    : \x1b[1;32mActivo\x1b[0m [{} - PID: {}]",
                        provider.as_str(),
                        pid
                    );
                }
                TunnelStatus::Stopped {
                    configured: Some(provider),
                } => {
                    println!(
                        "  Túnel de Red    : \x1b[1;33mDetenido\x1b[0m ({})",
                        provider.as_str()
                    );
                }
                TunnelStatus::Stopped { configured: None } | TunnelStatus::NotConfigured => {
                    println!("  Túnel de Red    : No configurado");
                }
            }

            println!("\n\x1b[1;36m[ Telemetría del Hardware y Kernel ]\x1b[0m");
            let telemetry = android::collect_telemetry()
                .context("Fallo al recolectar telemetría de hardware desde sysfs/procfs")?;

            if let Some(uptime) = telemetry.uptime_seconds {
                let hours = uptime / 3600;
                let minutes = (uptime % 3600) / 60;
                println!("  Uptime Kernel   : {}h {}m ({}s)", hours, minutes, uptime);
            }

            if let Some(mem) = telemetry.memory {
                println!(
                    "  Memoria RAM     : {} MB / {} MB ({}% en uso, {} MB libres)",
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
                        "  Batería ({}) : {}% [{}]{}{}",
                        bat.supply_name, bat.capacity_percent, bat.status, temp_info, volt_info
                    );
                }
                None => {
                    println!("  Batería         : No detectada (entorno sin power_supply físico)");
                }
            }

            match telemetry.max_soc_temp_c {
                Some(max_temp) => {
                    println!(
                        "  Temperatura SoC : {:.1} °C máx ({} zonas térmicas activas)",
                        max_temp,
                        telemetry.thermal_zones.len()
                    );
                }
                None => {
                    println!("  Temperatura SoC : Sin sensores térmicos reportados en sysfs");
                }
            }
        }
    }

    Ok(())
}