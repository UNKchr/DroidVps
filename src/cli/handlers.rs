use crate::cli::args::{Commands, BANNER};
use crate::container;
use crate::rootfs;
use anyhow::{Context, Result};
use std::path::Path;

/// Imprime el banner oficial de droidvps en la consola.
/// Utiliza la constante estática compilada en args.rs.
pub fn print_banner() {
    println!("{}", BANNER);
}

/// Enruta y procesa los subcomandos de la CLI, inyectando contexto de error.
pub fn handle_command(command: Commands) -> Result<()> {
    // Garantizamos que la identidad visual se imprima antes de cada operación
    print_banner();

    match command {
        Commands::Init { path } => {
            println!("\x1b[1;34m[*]\x1b[0m Starting procurement in: {}", path);
            let rootfs_path = Path::new(&path);
            
            rootfs::setup_rootfs(rootfs_path)
                .with_context(|| format!("Error while mounting the root filesystem at {}", path))?;
                
            println!("\x1b[1;32m[+]\x1b[0m Procurement completed successfully.");
        }
        
        Commands::Enter { path } => {
            println!("\x1b[1;34m[*]\x1b[0m Setting up the environment and entering the container in: {}", path);
            let rootfs_path = Path::new(&path);
            
            container::run_shell(rootfs_path)
                .with_context(|| format!("Critical error when running the shell or during unmounting at {}", path))?;
                
            println!("\x1b[1;32m[+]\x1b[0m Safe unmounting completed. Session terminated.");
        }
        
        Commands::Start { daemon } => {
            if daemon {
                println!("\x1b[1;33m[Stub]\x1b[0m Starting droidvps in the background (daemon)...");
            } else {
                println!("\x1b[1;33m[Stub]\x1b[0m Starting droidvps services in the foreground...");
            }
        }
        
        Commands::Stop => {
            println!("\x1b[1;33m[Stub]\x1b[0m Shutting down services and performing a safe shutdown...");
        }
        
        Commands::Status => {
            println!("\x1b[1;33m[Stub]\x1b[0m Checking telemetry and container status...");
        }
    }

    Ok(())
}