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

**DroidVPS** is a low-level container orchestrator written in **Rust (100% static, zero-bloat)** designed to transform old or obsolete rooted Android devices into lightweight, secure, and stable *headless* Linux servers (VPS).

Unlike solutions based on emulation (`PRoot` / `QEMU`) or custom ROM flashing (`postmarketOS`), **DroidVPS** operates directly on Android's existing Linux kernel through native `chroot` and idempotent *bind mounts*. This isolates a complete **Alpine Linux Minimal (`aarch64`)** environment at native CPU speed while preserving the manufacturer's proprietary drivers (Wi-Fi, LTE modem, charging management, and thermal sensors).

---

## Main Technical Features

* **Standalone Static Binary (`aarch64-unknown-linux-musl`):** Compiled against `musl libc` with size optimization (`opt-level = "z"`, `LTO`, `strip`), with no dependency on Android's `Bionic` library.
* **Transparent Android Kernel Patches:**
  * **`CONFIG_ANDROID_PARANOID_NETWORK` bypass:** Automatically injects the supplementary groups `AID_INET` (`3003`) and `AID_NET_RAW` (`3004`) at process level (`setgroups`) and persistently in `/etc/group` inside the container during initialization.
  * **Power Management (`wakelocks`):** Directly acquires and releases `/sys/power/wake_lock` and `/sys/power/wake_unlock` to prevent deep sleep when the screen is off.
  * **SELinux control:** Safely verifies and switches to permissive mode through `/sys/fs/selinux/enforce` while initializing subsystems.
* **Idempotent Mount Manager (`MountManager`):**
  * Actively inspects `/proc/mounts` to prevent duplicate mounts (*overmounting*).
  * Applies `MS_SLAVE | MS_REC` to `/dev` and `/sys` to prevent mount event leaks into Android's global namespace.
  * Safely lazy-unmounts (`MNT_DETACH`) in reverse order and intelligently preserves resources when interactive sessions (`enter`) coexist with background daemons (`start`).
* **Integrated VPS Services and Tunnel Assistant:**
  * **Dropbear SSH:** Unattended `apk` installation, automatic host-key generation (`-R`), initial credential setup, and live-process validation through `/proc/<pid>/cmdline`.
  * **Smart Tunnel Assistant (`droidvps tunnel`):** Automatic **Tailscale** provisioning (`--tun=userspace-networking`, avoiding conflicts with Android `netd` and `VpnService`) and automatic download/configuration of the official **Cloudflare Tunnel (`cloudflared` `linux-arm64`)** binary.
* **Real-Time Hardware Telemetry:** Resilient, normalized readings of battery capacity, charging status, voltage, and temperature (`/sys/class/power_supply`), SoC thermal sensors (`/sys/class/thermal`), and RAM (`/proc/meminfo`).

---

## Project Architecture

~~~text
droidvps/
├── .cargo/
│   └── config.toml          # aarch64-linux-musl-gcc cross-linker configuration
├── Cargo.toml               # Zero-bloat dependency manifest and release profile
├── Cross.toml               # Cross build configuration
├── CHANGELOG.md             # Project version history
├── LICENSE.md               # MIT license
├── README.md                # Technical documentation and user guide
└── src/
    ├── main.rs              # CLI binary entry point
    ├── lib.rs               # Library module exports
    ├── error.rs             # Centralized DroidError enum (thiserror)
    ├── cli.rs               # Command-line interface facade
    ├── cli/
    │   ├── args.rs          # Declarative argument and subcommand definitions (Clap)
    │   └── handlers.rs      # Command dispatcher and ANSI output formatting
    ├── rootfs.rs            # Alpine Linux, DNS, and Android group provisioning
    ├── rootfs/
    │   ├── download.rs      # Streaming HTTP download (ureq) + SHA256 verification
    │   └── extract.rs       # tar.gz extraction preserving POSIX permissions
    ├── container.rs         # VPS lifecycle (run_shell, start_vps, stop_vps, status)
    ├── container/
    │   ├── mount.rs         # Idempotent MountManager (/dev, /dev/pts, /proc, /sys)
    │   └── chroot.rs        # ChrootRunner (pre_exec, setsid, shells, and daemons)
    ├── android.rs           # Android-specific subsystem facade
    ├── android/
    │   ├── network.rs       # Android kernel group injection (AID_INET / AID_NET_RAW)
    │   ├── power.rs         # Wakelock control in /sys/power/
    │   ├── selinux.rs       # SELinux state management in /sys/fs/selinux/enforce
    │   └── telemetry.rs     # Normalized sysfs readings (battery, thermal zones, and RAM)
    └── services.rs          # Server daemon facade
        ├── ssh.rs           # Dropbear SSH server provisioning and management
        └── tunnel.rs        # Tailscale and Cloudflare Tunnel installation and orchestration
~~~

---

## Prerequisites

### On the Android Device (Target)
1. **Architecture:** ARM64 (`aarch64`).
2. **Root access:** Magisk, KernelSU, or APatch (always run as the `su` superuser).
3. **Connectivity:** Active Wi-Fi or mobile data for initial Alpine Linux and package provisioning.

### In the Development Environment (Build)
1. **Operating system:** Native Linux or **Windows 11 with WSL2 (Ubuntu)**.
2. **Rust toolchain:** `rustup` with the static target installed:
   ~~~bash
   rustup target add aarch64-unknown-linux-musl
   ~~~
3. **Musl cross-linker:** `aarch64-linux-musl-cross` toolchain available in `PATH` (`aarch64-linux-musl-gcc`).

---

## Build and Installation

### 1. Build the Static Binary
From the project root on Linux / WSL2:

~~~bash
cargo build --release
~~~

The optimized binary will be generated at:
~~~text
target/aarch64-unknown-linux-musl/release/droidvps
~~~

Verify that the executable is 100% static by running:
~~~bash
file target/aarch64-unknown-linux-musl/release/droidvps
# Expected output: ELF 64-bit LSB executable, ARM aarch64, version 1 (SYSV), statically linked, stripped
~~~

### 2. Deploy to the Android Device via ADB
Connect the phone over USB (or wireless ADB) and transfer the binary:

~~~bash
adb push target/aarch64-unknown-linux-musl/release/droidvps /data/local/tmp/droidvps
adb shell su -c "mkdir -p /data/local/droidvps/bin && \
                 mv /data/local/tmp/droidvps /data/local/droidvps/bin/droidvps && \
                 chmod 0755 /data/local/droidvps/bin/droidvps && \
                 ln -sf /data/local/droidvps/bin/droidvps /data/local/droidvps/bin/dvps"
~~~

*(Optional)* Add `/data/local/droidvps/bin` to your `PATH`, or run it directly from an `su` shell.

---

## Usage Guide

### 1. Initialize the Base Environment (`init`)
Downloads Alpine Linux Minimal `aarch64`, verifies its `SHA256` cryptographic hash, extracts the system while preserving POSIX permissions, configures `/etc/resolv.conf` and `/etc/hosts`, and injects Android network groups (`3003`/`3004`) into `/etc/group`:

~~~bash
droidvps init
~~~
*(By default, it is deployed at `/data/local/droidvps/rootfs`. Use `--target <PATH>` to customize the location.)*

### 2. Start the VPS Server in the Background (`start`)
Configures SELinux, acquires the kernel wakelock, mounts pseudo-filesystems, installs/starts **Dropbear SSH**, and automatically starts any previously configured tunnel:

~~~bash
droidvps start
~~~
* **Default SSH port:** `2222`
* **Initial user:** `root`
* **Initial password:** `droidvps` *(Change it after the first start by running `passwd`.)*

To connect from a PC on the same local network:
~~~bash
ssh -p 2222 root@<IP_DEL_ANDROID>
~~~

### 3. Configure Remote Access with the Tunnel Assistant (`tunnel`)
You do not need to enter the container manually or download binaries yourself; the `droidvps tunnel` assistant installs and configures everything for you:

* **Option A: Tailscale (Interactive or with an auth key)**
  Automatically installs the `tailscale` package through `apk` if missing, starts `tailscaled` in `--tun=userspace-networking` mode, and runs `tailscale up`:
  ~~~bash
  # Interactive mode (shows the authentication link in the console):
  droidvps tunnel tailscale

  # Unattended mode with a pre-generated key:
  droidvps tunnel tailscale --auth-key tskey-auth-xxxxx
  ~~~

* **Option B: Cloudflare Tunnel (`cloudflared`)**
  Automatically downloads the official `cloudflared-linux-arm64` binary to `/usr/local/bin/cloudflared` (if not installed), stores the token with `0600` permissions, and starts the tunnel in the background:
  ~~~bash
  droidvps tunnel cloudflared --token <YOUR_CLOUDFLARE_TOKEN>
  ~~~

* **Stop only the active tunnel:**
  ~~~bash
  droidvps tunnel stop
  ~~~
  *(Note: Once either tunnel is configured, running `droidvps start` automatically starts the tunnel together with Dropbear SSH.)*

### 4. Open a Direct Interactive Console (`enter`)
Enters an isolated `/bin/sh -l` shell directly inside the container without using SSH. If the VPS server is running in the background, services and mounts remain active after leaving the shell:

~~~bash
droidvps enter
~~~

### 5. Check Status and Telemetry (`status`)
Shows `rootfs` initialization status, active mount points in `/proc/mounts`, Dropbear SSH status and PID, tunnel status, RAM usage, *uptime*, and battery/SoC temperature metrics:

~~~bash
droidvps status
~~~

### 6. Stop the VPS Node (`stop`)
Safely stops tunnel and SSH daemons, unmounts `/sys`, `/proc`, `/dev/pts`, and `/dev` using `MNT_DETACH`, and releases the Android kernel wakelock:

~~~bash
droidvps stop
~~~

---

## License

Distributed under the **MIT** license. See [`LICENSE.md`](LICENSE.md) for more information.