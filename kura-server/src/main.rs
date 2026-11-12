//! Standalone server binary, for hosting a library without the TUI's
//! dependencies. `kura serve` runs the same code with the same options.

use std::path::PathBuf;

use clap::Parser;
use kura_core::{ConfigFile, LibraryPaths};
use kura_server::ServeArgs;

#[derive(Parser)]
#[command(version, about = "Serve a Kura library over HTTP")]
struct Args {
    /// Keep the whole library (images, database, thumbnails) under this folder
    /// instead of the current user's standard folders.
    #[arg(long)]
    library: Option<PathBuf>,

    #[command(flatten)]
    serve: ServeArgs,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = Args::parse();
    let config = ConfigFile::locate()?.load()?;
    kura_server::run(LibraryPaths::resolve(args.library.as_deref(), &config)?, args.serve).await
}
