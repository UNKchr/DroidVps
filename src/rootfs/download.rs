use crate::error::DroidError;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

/// Descarga el tarball del rootfs de forma síncrona y verifica su integridad SHA256.
/// Se usa lectura por bloques (chunks) para no cargar todo el archivo en la RAM del dispositivo.
pub fn download_and_verify(
    url: &str,
    expected_sha256: &str,
    dest_path: &Path,
) -> Result<(), DroidError> {
    // 1. Iniciamos la petición HTTP bloqueante
    let response = ureq::get(url).call().map_err(|e| {
        DroidError::Rootfs(format!("HTTP error when downloading rootfs: {}", e))
    })?;

    let mut reader = response.into_reader();
    let mut file = File::create(dest_path)?; // `?` delega automáticamente a DroidError::Io
    
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192]; // Buffer de 8KB para streaming eficiente

    // 2. Leemos, hasheamos y escribimos a disco simultáneamente (Tee streaming)
    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break; 
        }
        
        hasher.update(&buffer[..bytes_read]);
        file.write_all(&buffer[..bytes_read])?;
    }

    // 3. Verificamos la integridad criptográfica
    let hash_result = hasher.finalize();
    let hash_hex = format!("{:02x}", hash_result);

    if hash_hex != expected_sha256 {
        // Eliminamos el archivo corrupto para evitar que extract.rs intente usarlo
        let _ = std::fs::remove_file(dest_path);
        
        return Err(DroidError::Rootfs(format!(
            "Integrity check failed for SHA256. Expected: {}, Computed: {}",
            expected_sha256, hash_hex
        )));
    }

    Ok(())
}