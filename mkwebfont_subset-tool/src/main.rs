mod download_common_crawl;
mod legacy_gfsubsets;
mod test_subsetting_quality;

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
    DownloadCommonCrawl,
    TestSubsettingQuality,
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
        Commands::DownloadCommonCrawl => download_common_crawl::download_common_crawl()
            .await
            .unwrap(),
        Commands::TestSubsettingQuality => {
            test_subsetting_quality::test_subsetting_quality().unwrap()
        }
    }
}
