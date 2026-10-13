pub mod download;
pub mod extract;

use crate::error::DroidError;
use std::fs;
use std::path::Path;

// Constantes de aprovisionamiento.
// Nota: Puedes actualizar la URL y el SHA256 a la versión más reciente de Alpine.
pub const ALPINE_URL: &str = "https://dl-cdn.alpinelinux.org/alpine/v3.19/releases/aarch64/alpine-minirootfs-3.19.1-aarch64.tar.gz";
pub const ALPINE_SHA256: &str = "7e6ebfecfc7fc2d9bc9b462c19e59d919bb14ea7bf188049f532a893ef15b810";
pub const DEFAULT_ROOTFS_PATH: &str = "/data/local/droidvps/rootfs";

/// Orquesta la descarga, verificación, extracción y configuración inicial del rootfs.
pub fn setup_rootfs(rootfs_path: &Path) -> Result<(), DroidError> {
    // 1. Detección de instalación previa
    let bin_sh = rootfs_path.join("bin/sh");
    if bin_sh.exists() {
        // Ya está instalado, omitimos el aprovisionamiento.
        return Ok(());
    }

    // 2. Preparación del directorio
    if let Some(parent) = rootfs_path.parent() {
        fs::create_dir_all(parent)?;
    }
    
    let tarball_path = rootfs_path.with_extension("tar.gz");

    // 3. Descarga y Verificación
    download::download_and_verify(ALPINE_URL, ALPINE_SHA256, &tarball_path)?;

    // 4. Extracción
    extract::extract_tarball(&tarball_path, rootfs_path)?;

    // Limpiamos el archivo tar.gz comprimido para no desperdiciar espacio en /data
    let _ = fs::remove_file(&tarball_path);

    // 5. Configuración Post-Instalación de Red (DNS y Hosts)
    let etc_dir = rootfs_path.join("etc");
    if !etc_dir.exists() {
        fs::create_dir_all(&etc_dir)?;
    }

    // Escribimos resolv.conf: El Bionic libc de Android maneja DNS distinto (vía netd),
    // por lo que el chroot de Musl/Alpine necesita resolvers estáticos explícitos.
    let resolv_conf = etc_dir.join("resolv.conf");
    fs::write(&resolv_conf, "nameserver 1.1.1.1\nnameserver 8.8.8.8\n")?;

    // Escribimos hosts: Necesario para que herramientas de red resuelvan el bucle local
    let hosts = etc_dir.join("hosts");
    fs::write(&hosts, "127.0.0.1\tlocalhost\n::1\tlocalhost\n")?;

    Ok(())
}