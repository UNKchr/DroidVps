# Changelog

All notable changes to **`droidvps`** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [0.1.0] - 2026-10-13

### Added
- **CLI interface (`src/cli/`):**
  - Declarative `clap` parser supporting `init`, `enter`, `start`, `stop`, `status`, and the `tunnel` assistant (`tailscale`, `cloudflared`, `stop`).
  - Official `droidvps` ASCII banner with native ANSI sequences and author credit (`UNKchr`).
- **Rootfs provisioning (`src/rootfs/`):**
  - Synchronous streaming download (`ureq`) of Alpine Linux Minimal (`aarch64`) with `SHA256` cryptographic verification (`sha2`).
  - `.tar.gz` extraction (`flate2` + `tar`) preserving POSIX permissions and metadata.
  - Automatic generation of `/etc/resolv.conf`, `/etc/hosts`, and persistent Android network groups (`aid_inet:3003` and `aid_net_raw:3004`) in `/etc/group`.
- **Container engine and isolation (`src/container/`):**
  - Idempotent `MountManager` with `/proc/mounts` verification, `MS_SLAVE | MS_REC` propagation isolation, and lazy unmounting (`MNT_DETACH`).
  - `ChrootRunner` based on `CommandExt::pre_exec` and `setsid` for isolated interactive shells, synchronous commands, and background daemons without trapping the parent process.
  - Intelligent mount and *wakelock* preservation when leaving `enter` sessions while background daemons are active.
- **Android kernel compatibility (`src/android/`):**
  - Supplementary `AID_INET` (`3003`) and `AID_NET_RAW` (`3004`) groups to allow TCP/UDP/ICMP sockets under `CONFIG_ANDROID_PARANOID_NETWORK`.
  - Power-lock acquisition and release through `/sys/power/wake_lock` and `/sys/power/wake_unlock`.
  - SELinux state management through `/sys/fs/selinux/enforce`.
  - Telemetry subsystem (`telemetry.rs`) with resilient readings and automatic scale normalization for battery (`/sys/class/power_supply`), SoC thermal sensors (`/sys/class/thermal`), RAM (`/proc/meminfo`), and *uptime* (`/proc/uptime`).
- **VPS services and tunnel orchestration (`src/services/`):**
  - `SshService`: unattended Dropbear SSH installation, persistent Android network groups in `/etc/group`, initial `root` password configuration, and PID validation against `/proc/<pid>/cmdline`.
  - `TunnelService`: automated `tailscaled` assistant (`apk` installation, `--tun=userspace-networking`, and interactive or `--auth-key` authentication) and `cloudflared` support (streaming download of the official `linux-arm64` binary, secure `0600` token storage, and direct execution without intermediate shells).