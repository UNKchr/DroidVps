# Changelog

All notable changes to the **`droidvps`** project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-10-13

### Added
- **CLI Interface (`src/cli/`):**
  - Declarative `clap` parser supporting `init`, `enter`, `start`, `stop`, `status`, `setup`, and `tunnel` (`tailscale`, `cloudflared`, `stop`) subcommands.
  - Official `droidvps` ASCII banner with native ANSI escape sequences and author credits (`UNKchr`).
- **Rootfs Provisioning (`src/rootfs/`):**
  - Synchronous streaming HTTP download (`ureq`) of Alpine Linux Minimal (`aarch64`) with cryptographic `SHA256` verification (`sha2`).
  - `.tar.gz` extraction (`flate2` + `tar`) preserving POSIX metadata, permissions, and symlinks.
  - Automated generation of `/etc/resolv.conf`, `/etc/hosts`, and preventive registration of Android kernel network groups (`aid_inet:3003` and `aid_net_raw:3004`) in `/etc/group`.
- **Container & Isolation Engine (`src/container/`):**
  - Idempotent `MountManager` with `/proc/mounts` inspection, `MS_SLAVE | MS_REC` mount propagation isolation, and lazy unmounting (`MNT_DETACH`).
  - `ChrootRunner` using `CommandExt::pre_exec` and `setsid` for isolated interactive shells, synchronous commands, and detached background daemons.
  - Smart resource preservation on `enter` shell exit when background VPS daemons are active.
- **Android Kernel Compatibility & Standalone Environment (`src/android/`):**
  - `env.rs`: Root privilege verification (`UID == 0`), standalone installation in `/data/local/droidvps/bin`, automatic Termux wrapper generation (`droidvps` and `dvps` with `su -mm` elevation), and Magisk/KernelSU/APatch boot autostart script management in `/data/adb/service.d/`.
  - `network.rs`: Supplementary group injection (`AID_INET` `3003` and `AID_NET_RAW` `3004`) to allow socket creation under `CONFIG_ANDROID_PARANOID_NETWORK`.
  - `power.rs`: Kernel wakelock acquisition and release via `/sys/power/wake_lock` and `/sys/power/wake_unlock`.
  - `selinux.rs`: SELinux state inspection and permissive mode transition via `/sys/fs/selinux/enforce`.
  - `telemetry.rs`: Resilient sysfs/procfs hardware telemetry collector with automatic unit normalization for battery (`/sys/class/power_supply`), thermal zones (`/sys/class/thermal`), RAM (`/proc/meminfo`), and uptime (`/proc/uptime`).
- **VPS Services & Smart Tunnel Assistant (`src/services/`):**
  - `SshService`: Unattended Dropbear SSH installation, Android `/etc/group` persistence, default `root` password provisioning, and live PID verification via `/proc/<pid>/cmdline`.
  - `TunnelService`: Automated assistant for `tailscaled` (`apk` provisioning, `--tun=userspace-networking` daemon startup, and interactive or `--auth-key` login) and `cloudflared` (streaming download of official `linux-arm64` binary, `0600` token storage, and direct daemon execution).
- **Automated Test Suite (`tests/integration_tests.rs`):**
  - Unit and integration tests covering CLI parsing, flag conflicts, idempotent `/etc/group` patching, stale PID detection, token validation, Termux wrapper generation, boot autostart lifecycle, and telemetry collection.