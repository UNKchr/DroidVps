use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;
use droidvps::android;
use droidvps::cli::args::{Cli, Commands, TunnelAction};
use droidvps::container;
use droidvps::rootfs::{configure_android_groups, DEFAULT_ROOTFS_PATH};
use droidvps::services::{
    SshService, TunnelProvider, TunnelService, TunnelStatus, DEFAULT_SSH_PORT,
};

/// Crea un directorio temporal único para pruebas aisladas sin requerir permisos root
fn create_temp_rootfs(test_name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Reloj del sistema válido")
        .as_nanos();
    let dir = env::temp_dir().join(format!(
        "droidvps_itest_{}_{}_{}",
        test_name,
        std::process::id(),
        nanos
    ));
    fs::create_dir_all(&dir).expect("Debe crear directorio temporal de prueba");
    dir
}

#[test]
fn test_cli_default_and_custom_targets() {
    let cli = Cli::try_parse_from(["droidvps", "init"]).expect("Debe parsear subcomando init");
    assert_eq!(
        cli.command,
        Commands::Init {
            target: PathBuf::from(DEFAULT_ROOTFS_PATH)
        }
    );

    let custom = "/data/local/custom_rootfs";
    let cli = Cli::try_parse_from(["droidvps", "enter", "--target", custom])
        .expect("Debe parsear enter con --target personalizado");
    assert_eq!(
        cli.command,
        Commands::Enter {
            target: PathBuf::from(custom)
        }
    );
}

#[test]
fn test_cli_tunnel_subcommands_parsing() {
    // Tailscale interactivo (sin auth-key)
    let cli = Cli::try_parse_from(["droidvps", "tunnel", "tailscale"])
        .expect("Debe parsear tunnel tailscale interactivo");
    assert_eq!(
        cli.command,
        Commands::Tunnel {
            action: TunnelAction::Tailscale {
                target: PathBuf::from(DEFAULT_ROOTFS_PATH),
                auth_key: None,
            }
        }
    );

    // Tailscale desatendido (con auth-key)
    let cli = Cli::try_parse_from([
        "droidvps",
        "tunnel",
        "tailscale",
        "--auth-key",
        "tskey-auth-xyz",
    ])
    .expect("Debe parsear tunnel tailscale con auth-key");
    assert_eq!(
        cli.command,
        Commands::Tunnel {
            action: TunnelAction::Tailscale {
                target: PathBuf::from(DEFAULT_ROOTFS_PATH),
                auth_key: Some("tskey-auth-xyz".to_string()),
            }
        }
    );

    // Cloudflared sin --token debe ser rechazado por Clap
    assert!(Cli::try_parse_from(["droidvps", "tunnel", "cloudflared"]).is_err());

    // Cloudflared con --token válido
    let cli = Cli::try_parse_from([
        "droidvps",
        "tunnel",
        "cloudflared",
        "--token",
        "eyJhIjoiMTIzNDUifQ==",
    ])
    .expect("Debe parsear tunnel cloudflared con token");
    assert_eq!(
        cli.command,
        Commands::Tunnel {
            action: TunnelAction::Cloudflared {
                target: PathBuf::from(DEFAULT_ROOTFS_PATH),
                token: "eyJhIjoiMTIzNDUifQ==".to_string(),
            }
        }
    );
}

#[test]
fn test_rootfs_android_groups_injection_is_idempotent() {
    let rootfs = create_temp_rootfs("android_groups");
    let etc_dir = rootfs.join("etc");
    fs::create_dir_all(&etc_dir).unwrap();

    let group_file = etc_dir.join("group");
    fs::write(&group_file, "root:x:0:root\nwheel:x:10:root\n").unwrap();

    // 1ra pasada: inyecta 3003 y 3004
    configure_android_groups(&rootfs).expect("Primera inyección de grupos Android");
    let first = fs::read_to_string(&group_file).unwrap();
    assert!(first.contains("aid_inet:x:3003:root"));
    assert!(first.contains("aid_net_raw:x:3004:root"));

    // 2da pasada: no debe duplicar entradas
    configure_android_groups(&rootfs).expect("Segunda inyección idempotente");
    let second = fs::read_to_string(&group_file).unwrap();
    assert_eq!(first, second);
    assert_eq!(second.matches(":3003:").count(), 1);
    assert_eq!(second.matches(":3004:").count(), 1);

    let _ = fs::remove_dir_all(&rootfs);
}

#[test]
fn test_container_status_and_uninitialized_guard() {
    let rootfs = create_temp_rootfs("container_status");

    // 1. Directorio vacío -> rootfs_initialized debe ser false y run_shell/start_vps deben fallar
    let initial_status = container::inspect_status(&rootfs);
    assert!(!initial_status.rootfs_initialized);
    assert!(initial_status.active_mounts.is_empty());
    assert_eq!(initial_status.ssh_pid, None);
    assert_eq!(initial_status.ssh_port, DEFAULT_SSH_PORT);
    assert_eq!(initial_status.tunnel_status, TunnelStatus::NotConfigured);

    assert!(container::run_shell(&rootfs).is_err());
    assert!(container::start_vps(&rootfs).is_err());

    // 2. Simular estructura mínima de Alpine Linux (/bin/sh y /etc/alpine-release)
    fs::create_dir_all(rootfs.join("bin")).unwrap();
    fs::create_dir_all(rootfs.join("etc")).unwrap();
    fs::write(rootfs.join("bin/sh"), b"#!/bin/sh\n").unwrap();
    fs::write(rootfs.join("etc/alpine-release"), b"3.19.1\n").unwrap();

    let ready_status = container::inspect_status(&rootfs);
    assert!(ready_status.rootfs_initialized);

    let _ = fs::remove_dir_all(&rootfs);
}

#[test]
fn test_services_stale_pid_detection_and_tunnel_states() {
    let rootfs = create_temp_rootfs("services_state");
    fs::create_dir_all(rootfs.join("run")).unwrap();
    fs::create_dir_all(rootfs.join("etc/droidvps")).unwrap();

    // 1. Escribir un PID inexistente (999999) en /run/dropbear.pid
    fs::write(rootfs.join("run/dropbear.pid"), "999999\n").unwrap();
    let ssh = SshService::new(&rootfs);
    assert!(!ssh.is_running());
    assert_eq!(ssh.running_pid(), None);

    // 2. Estado inicial del túnel -> NotConfigured
    let tunnel = TunnelService::new(&rootfs);
    assert_eq!(tunnel.status(), TunnelStatus::NotConfigured);

    // 3. Habilitar marcador de Tailscale -> Stopped { configured: Some(Tailscale) }
    fs::write(rootfs.join("etc/droidvps/tailscale.enabled"), "enabled\n").unwrap();
    assert_eq!(
        tunnel.status(),
        TunnelStatus::Stopped {
            configured: Some(TunnelProvider::Tailscale)
        }
    );

    // 4. Registrar token de Cloudflared -> tiene prioridad como proveedor configurado
    fs::write(
        rootfs.join("etc/droidvps/cloudflared.token"),
        "eyJhIjoiMTIzNDUifQ==\n",
    )
    .unwrap();
    assert_eq!(
        tunnel.status(),
        TunnelStatus::Stopped {
            configured: Some(TunnelProvider::Cloudflared)
        }
    );

    // 5. Token malicioso con caracteres de inyección de comandos debe ser rechazado
    let bad_token_result =
        tunnel.setup_and_start_cloudflared("token_invalido; rm -rf /");
    assert!(bad_token_result.is_err());

    let _ = fs::remove_dir_all(&rootfs);
}

#[test]
fn test_telemetry_collection_graceful_on_any_linux_host() {
    let telemetry = android::collect_telemetry()
        .expect("collect_telemetry no debe fallar en WSL2 ni en Android");

    // En cualquier kernel Linux (/proc/meminfo y /proc/uptime existen), debe reportar RAM y Uptime
    assert!(telemetry.memory.is_some());
    assert!(telemetry.uptime_seconds.is_some());
}