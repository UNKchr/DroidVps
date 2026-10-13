use clap::Parser;
use droidvps::cli::{handle_command, Cli};

fn main() -> anyhow::Result<()> {
    let args = Cli::parse();
    handle_command(args.command)?;
    Ok(())
}
