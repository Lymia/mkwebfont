mod legacy_gfsubsets;

use clap::{Parser, Subcommand};
use std::io;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    GenerateLegacyGfsubsets,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_env_filter("debug,h2=info,hyper_util=info,reqwest=info,rustls=info")
        .with_writer(io::stderr)
        .init();
    match args.command {
        Commands::GenerateLegacyGfsubsets => legacy_gfsubsets::main().await,
    }
}
