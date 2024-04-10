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

    /// Downloads about 260GB of common crawl data and processes it into bitsets of characters
    /// present in each crawled page. This must be called before any of the commands in this crate
    /// for generating subset data.
    ///
    /// This is **NOT** required for anything a user may want to do normally, and is only useful
    /// for developing mkwebfont typically.
    ///
    /// This takes about 270 gigabytes of disk space (as it caches Common Crawl data to disk) and
    /// a relatively large amount of memory as it stores the generated bitset data uncompressed in
    /// memory while waiting to compress it (on the order of 10-20GB from testing).
    ///
    /// The Common Crawl raw dumps are not required for any other commands. They may be deleted
    /// after `common-crawl_parsed-bitmaps.zst` and `common-crawl-validation_parsed-bitmaps.zst`
    /// are generated. These files should total to about 4 GB.
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
