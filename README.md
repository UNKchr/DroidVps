# DroidVPS (`droidvps`)

~~~text
██████╗ ██████╗  ██████╗ ██╗██████╗ ██╗   ██╗██████╗ ███████╗
██╔══██╗██╔══██╗██╔═══██╗██║██╔══██╗██║   ██║██╔══██╗██╔════╝
██║  ██║██████╔╝██║   ██║██║██║  ██║██║   ██║██████╔╝███████╗
██║  ██║██╔══██╗██║   ██║██║██║  ██║╚██╗ ██╔╝██╔═══╝ ╚════██║
██████╔╝██║  ██║╚██████╔╝██║██████╔╝ ╚████╔╝ ██║     ███████║
╚═════╝ ╚═╝  ╚═╝ ╚═════╝ ╚═╝╚═════╝   ╚═══╝  ╚═╝     ╚══════╝
  v0.1.0 • Developed by UNKchr
~~~

**DroidVPS** is a low-level, zero-bloat container orchestrator written in **Rust (100% static)** designed to repurpose old or obsolete rooted Android devices into lightweight, stable, and secure headless Linux servers (VPS).

Unlike solutions that rely on syscall emulation (`PRoot` / `QEMU`) or flashing custom ROMs (`postmarketOS`), **DroidVPS** runs directly on top of the existing Android Linux kernel using native `chroot` isolation and idempotent bind mounts. It orchestrates a complete **Alpine Linux Minimal (`aarch64`)** userland at native CPU speed while retaining all proprietary hardware drivers (Wi-Fi, LTE modem, battery management, and thermal sensors).

---

## Key Technical Features

* **100% Static Binary (`aarch64-unknown-linux-musl`):** Compiled against `musl libc` with size optimizations (`opt-level = "z"`, `LTO`, `strip`), completely free of Android `Bionic libc` dependencies.
* **Transparent Android Kernel Fixes:**
  * **`CONFIG_ANDROID_PARANOID_NETWORK` Bypass:** Automatically injects Android supplementary groups `AID_INET` (`3003`) and `AID_NET_RAW` (`3004`) both at the process level (`setgroups`) and persistently in `<rootfs>/etc/group`.
  * **Power Management (`Wakelocks`):** Directly acquires and releases kernel wakelocks via `/sys/power/wake_lock` and `/sys/power/wake_unlock` to prevent the SoC from entering deep sleep when the screen is off.
  * **SELinux Management:** Safely checks and transitions `/sys/fs/selinux/enforce` to permissive mode when initializing container subsystems.
  * **Standalone Termux & Boot Persistence (`src/android/env.rs`):** Installs wrapper scripts (`droidvps` and `dvps`) into Termux's `$PREFIX/bin` using Magisk/KernelSU/APatch master mount namespace elevation (`su -mm`), and generates `/data/adb/service.d/` scripts for unattended startup on device boot.
* **Idempotent Mount Manager (`MountManager`):**
  * Inspects `/proc/mounts` prior to mounting `/dev`, `/dev/pts`, `/proc`, and `/sys` to prevent overmounting.
  * Applies `MS_SLAVE | MS_REC` propagation isolation so container mount events never leak into Android's global mount namespace.
  * Uses lazy unmounting (`MNT_DETACH`) and preserves active mounts and wakelocks when exiting interactive `enter` shells while background daemons (`start`) are running.
* **Built-in VPS Services & Smart Tunnel Assistant:**
  * **Dropbear SSH:** Unattended installation via `apk`, automatic host key generation (`-R`), initial `root` credential provisioning, and live process verification via `/proc/<pid>/cmdline`.
  * **Automated Network Tunnels (`droidvps tunnel`):** One-command setup and authentication for **Tailscale** (`--tun=userspace-networking`, avoiding conflicts with Android's `netd` and `VpnService`) and automatic binary download + token configuration for **Cloudflare Tunnel (`cloudflared` `linux-arm64`)**.
* **Real-Time Hardware Telemetry:** Resilient and normalized sysfs/procfs readers for battery capacity, charging status, voltage, and temperature (`/sys/class/power_supply`), SoC thermal zones (`/sys/class/thermal`), RAM usage (`/proc/meminfo`), and kernel uptime (`/proc/uptime`).

---

## Project Architecture

~~~text
droidvps/
├── .cargo/
│   └── config.toml          # Cross-linker configuration (aarch64-linux-musl-gcc)
├── Cargo.toml               # Zero-bloat dependencies and release profile optimizations
├── Cross.toml               # Cross compilation configuration
├── CHANGELOG.md             # Project version history
├── LICENSE.md               # MIT License
├── README.md                # Technical documentation and user guide
├── src/
│   ├── main.rs              # CLI binary entrypoint
│   ├── lib.rs               # Public library module exports
│   ├── error.rs             # Centralized DroidError enum (thiserror)
│   ├── cli.rs               # CLI facade
│   ├── cli/
│   │   ├── args.rs          # Declarative Clap parser and subcommands
│   │   └── handlers.rs      # Command dispatcher and ANSI output formatting
│   ├── rootfs.rs            # Alpine Linux provisioning, DNS, and Android groups
│   ├── rootfs/
│   │   ├── download.rs      # Streaming HTTP client (ureq) + SHA256 verification
│   │   └── extract.rs       # tar.gz extraction preserving POSIX permissions
│   ├── container.rs         # VPS lifecycle (run_shell, start_vps, stop_vps, inspect_status)
│   ├── container/
│   │   ├── mount.rs         # Idempotent MountManager (/dev, /dev/pts, /proc, /sys)
│   │   └── chroot.rs        # ChrootRunner (pre_exec, setsid, shells, and daemons)
│   ├── android.rs           # Android kernel subsystems facade
│   ├── android/
│   │   ├── env.rs           # Root check, Termux wrappers, and /data/adb/service.d autostart
│   │   ├── network.rs       # Kernel GID injection (AID_INET / AID_NET_RAW)
│   │   ├── power.rs         # Kernel Wakelock control in /sys/power/
│   │   ├── selinux.rs       # SELinux state management in /sys/fs/selinux/enforce
│   │   └── telemetry.rs     # Normalized sysfs/procfs hardware telemetry
│   └── services.rs          # Background daemons facade
│       ├── ssh.rs           # Dropbear SSH provisioning and lifecycle management
│       └── tunnel.rs        # Automated Tailscale and Cloudflare Tunnel orchestration
└── tests/
    └── integration_tests.rs # Integration and security test suite
~~~

---

## Prerequisites

### Target Android Device
1. **Architecture:** ARM64 (`aarch64`).
2. **Root Access:** Magisk, KernelSU, or APatch.
3. **Network:** Active Wi-Fi or mobile data connection for initial Alpine Linux provisioning.

### Development Host (Compilation)
1. **OS:** Linux or **Windows 11 with WSL2 (Ubuntu)**.
2. **Rust Toolchain:** `rustup` with the static musl target installed:
   ~~~bash
   rustup target add aarch64-unknown-linux-musl
   ~~~
3. **Musl Cross-Linker:** `aarch64-linux-musl-cross` toolchain in `PATH` (`aarch64-linux-musl-gcc`).

---

## Building & Testing

### 1. Run the Test Suite (Host CPU)
~~~bash
cargo test --target x86_64-unknown-linux-gnu
~~~

### 2. Build the Static Release Binary (`aarch64`)
~~~bash
cargo build --release
~~~

Verify the resulting binary at `target/aarch64-unknown-linux-musl/release/droidvps`:
~~~bash
file target/aarch64-unknown-linux-musl/release/droidvps
# Expected: ELF 64-bit LSB executable, ARM aarch64, version 1 (SYSV), statically linked, stripped
~~~

---

## Installation (ADB or Standalone via Termux)

### Option A: Direct Standalone Setup from Termux (No PC Required)
Copy the `droidvps` binary to your phone's storage (e.g., `/sdcard/Download/droidvps`), open **Termux**, and run:

~~~bash
su -mm -c "cp /sdcard/Download/droidvps /data/local/tmp/droidvps && \
           chmod 0755 /data/local/tmp/droidvps && \
           /data/local/tmp/droidvps setup --enable-autostart"
~~~

Once `setup` completes:
* The binary is installed at `/data/local/droidvps/bin/droidvps` (with symlink `dvps`).
* Termux wrappers (`droidvps` and `dvps`) are automatically created in `/data/data/com.termux/files/usr/bin`, allowing you to run `dvps status`, `dvps start`, or `dvps enter` directly from an unprivileged Termux session.
* A boot script (`99_droidvps_autostart.sh`) is registered in `/data/adb/service.d/` so the VPS starts automatically whenever the phone boots.

### Option B: Deployment from PC via ADB
~~~bash
adb push target/aarch64-unknown-linux-musl/release/droidvps /data/local/tmp/droidvps
adb shell su -mm -c "chmod 0755 /data/local/tmp/droidvps && /data/local/tmp/droidvps setup --enable-autostart"
~~~

---

## Usage Guide

### 1. Initialize the Base Rootfs (`init`)
Downloads Alpine Linux Minimal `aarch64`, verifies its `SHA256` checksum, extracts the archive preserving POSIX permissions, configures `/etc/resolv.conf` and `/etc/hosts`, and registers Android network groups (`3003`/`3004`) in `/etc/group`:

~~~bash
dvps init
~~~

### 2. Start the Headless VPS Node (`start`)
Configures SELinux, acquires the kernel Wakelock, mounts pseudo-filesystems, provisions/starts **Dropbear SSH**, and launches any configured network tunnel:

~~~bash
dvps start
~~~
* **Default SSH Port:** `2222`
* **Initial User:** `root`
* **Initial Password:** `droidvps` *(Change it after first login using `passwd`)*.

Connect from any machine on the local network:
~~~bash
ssh -p 2222 root@<ANDROID_IP>
~~~

### 3. Configure Remote Access Tunnels (`tunnel`)
The smart `tunnel` assistant automatically downloads, installs, and configures the required tunnel binaries inside the container:

* **Tailscale (Interactive or Auth-Key):**
  Automatically installs `tailscale` via `apk` if missing, starts `tailscaled` in `--tun=userspace-networking` mode, and runs `tailscale up`:
  ~~~bash
  # Interactive login (prints authentication URL):
  dvps tunnel tailscale

  # Unattended login with pre-generated key:
  dvps tunnel tailscale --auth-key tskey-auth-xxxxx
  ~~~

* **Cloudflare Tunnel (`cloudflared`):**
  Automatically downloads the official `cloudflared-linux-arm64` binary to `/usr/local/bin/cloudflared`, stores the token at `/etc/droidvps/cloudflared.token` (`0600`), and launches the daemon:
  ~~~bash
  dvps tunnel cloudflared --token <YOUR_CLOUDFLARE_TOKEN>
  ~~~

* **Stop Active Tunnel Only:**
  ~~~bash
  dvps tunnel stop
  ~~~

### 4. Open an Interactive Container Shell (`enter`)
Enters an isolated `/bin/sh -l` session directly. If background VPS daemons are active, exiting the shell preserves all mounts and wakelocks:

~~~bash
dvps enter
~~~

### 5. Inspect Node Status & Hardware Telemetry (`status`)
Displays rootfs state, active kernel mounts, Dropbear SSH PID/port, tunnel status, boot autostart state, RAM usage, uptime, battery metrics, and SoC temperature:

~~~bash
dvps status
~~~

### 6. Stop the VPS Node (`stop`)
Gracefully stops tunnel and SSH daemons, detaches all container mounts (`MNT_DETACH`), and releases the Android kernel Wakelock:

~~~bash
dvps stop
~~~

### 7. Manage Standalone Installation & Boot Autostart (`setup`)
~~~bash
# Enable boot autostart in /data/adb/service.d:
dvps setup --enable-autostart

# Disable boot autostart:
dvps setup --disable-autostart
~~~

---

## License

Distributed under the **MIT** License. See [`LICENSE.md`](LICENSE.md) for details.