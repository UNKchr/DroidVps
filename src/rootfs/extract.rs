use crate::error::DroidError;
use flate2::read::GzDecoder;
use std::fs::{self, File};
use std::path::Path;
use tar::Archive;

/// Descomprime y extrae el tarball preserving metadatos POSIX.
pub fn extract_tarball(archive_path: &Path, dest_dir: &Path) -> Result<(), DroidError> {
    if !archive_path.exists() {
        return Err(DroidError::Rootfs(format!(
            "The compressed file does not exist at the following path: {:?}",
            archive_path
        )));
    }

    // Aseguramos que el directorio destino exista antes de extraer
    if !dest_dir.exists() {
        fs::create_dir_all(dest_dir)?; // Se mapea a DroidError::Io
    }

    // 1. Abrimos el .tar.gz
    let tar_gz = File::open(archive_path)?;
    
    // 2. Configuramos el pipeline de descompresión: File -> GzDecoder -> tar::Archive
    let tar = GzDecoder::new(tar_gz);
    let mut archive = Archive::new(tar);

    // 3. Configuración estricta para chroot Linux
    // Es crítico preservar los permisos (ej. chmod +x, setuid) y los enlaces simbólicos 
    // de Alpine Linux (donde casi todos los binarios son symlinks a busybox).
    archive.set_preserve_permissions(true);
    archive.set_preserve_mtime(true);
    // archive.set_unpack_xattrs(true); // Opcional, pero suele ser problemático en particiones /data de Android

    // 4. Desempaquetamos en el destino
    archive.unpack(dest_dir).map_err(|e| {
        DroidError::Rootfs(format!("Extraction error (Partition without permissions / corrupted?): {}", e))
    })?;

    Ok(())
}