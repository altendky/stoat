// stoat: Streaming OAuth Transformer
//
// CLI entry point that wires stoat-core and stoat-io together.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// Streaming OAuth Transformer — a config-driven local reverse proxy for
/// OAuth token lifecycle management.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Perform the OAuth PKCE authorization code flow.
    Login {
        /// Path to the TOML config file.
        #[arg(long)]
        config: PathBuf,
    },

    /// Start the proxy server.
    Serve {
        /// Path to the TOML config file.
        #[arg(long)]
        config: PathBuf,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let config_path = match &cli.command {
        Commands::Login { config } | Commands::Serve { config } => config,
    };

    let toml_str = match stoat_io::read_file(config_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: failed to read config file: {e}");
            return ExitCode::FAILURE;
        }
    };

    let _config = match stoat_core::config::Config::from_toml(&toml_str) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: invalid config: {e}");
            return ExitCode::FAILURE;
        }
    };

    match &cli.command {
        Commands::Login { .. } => {
            eprintln!("error: `stoat login` is not yet implemented");
            ExitCode::FAILURE
        }
        Commands::Serve { .. } => {
            eprintln!("error: `stoat serve` is not yet implemented");
            ExitCode::FAILURE
        }
    }
}
