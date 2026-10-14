use std::net::UdpSocket;
use std::path::Path;
use std::sync::Once;
use std::thread;
use std::time::Duration;
use nix::unistd::{getgroups, setgroups, Gid};
use crate::error::DroidError;

/// `AID_INET`: Grupo del kernel Android requerido para crear sockets `AF_INET` / `AF_INET6`
pub const AID_INET: u32 = 3003;

/// `AID_NET_RAW`: Grupo del kernel Android requerido para crear sockets ICMP (`SOCK_RAW` / `ping`)
pub const AID_NET_RAW: u32 = 3004;

static HOST_DNS_INIT: Once = Once::new();

/// Inyecta los grupos suplementarios de red de Android en el proceso actual
/// y activa el puente DNS local (`127.0.0.1:53`) si el host Android carece de `/etc/resolv.conf`.
pub fn setup_android_network_groups() -> Result<(), DroidError> {
    let mut groups = getgroups()?;

    let inet_gid = Gid::from_raw(AID_INET);
    let net_raw_gid = Gid::from_raw(AID_NET_RAW);

    if !groups.contains(&inet_gid) {
        groups.push(inet_gid);
    }

    if !groups.contains(&net_raw_gid) {
        groups.push(net_raw_gid);
    }

    setgroups(&groups)?;
    ensure_host_dns_resolver();

    Ok(())
}

/// En Android no existe `/etc/resolv.conf`, por lo que `musl libc` envía todas las consultas
/// DNS de `getaddrinfo()` a `127.0.0.1:53`. Este puente escucha en `127.0.0.1:53` y reenvía
/// los paquetes UDP hacia Cloudflare (`1.1.1.1:53`) y Google (`8.8.8.8:53`).
pub fn ensure_host_dns_resolver() {
    HOST_DNS_INIT.call_once(|| {
        if Path::new("/etc/resolv.conf").exists() {
            return;
        }

        let Ok(local_socket) = UdpSocket::bind("127.0.0.1:53") else {
            return;
        };

        thread::spawn(move || {
            let mut buf = [0u8; 512];
            while let Ok((len, client_addr)) = local_socket.recv_from(&mut buf) {
                let query = buf[..len].to_vec();
                let Ok(reply_socket) = local_socket.try_clone() else {
                    continue;
                };

                // Procesar cada consulta (A / AAAA) en paralelo para evitar bloqueos en musl
                thread::spawn(move || {
                    let Ok(upstream) = UdpSocket::bind("0.0.0.0:0") else {
                        return;
                    };
                    let _ = upstream.set_read_timeout(Some(Duration::from_millis(2000)));
                    let _ = upstream.set_write_timeout(Some(Duration::from_millis(2000)));

                    for dns_server in ["1.1.1.1:53", "8.8.8.8:53"] {
                        if upstream.send_to(&query, dns_server).is_ok() {
                            let mut resp = [0u8; 512];
                            if let Ok((resp_len, _)) = upstream.recv_from(&mut resp) {
                                let _ = reply_socket.send_to(&resp[..resp_len], client_addr);
                                break;
                            }
                        }
                    }
                });
            }
        });
    });
}