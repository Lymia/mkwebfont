mod download_common_crawl;
mod generate_raw_adjacency;
mod legacy_gfsubsets;
mod split_common_crawl;
mod test_subsetting;
mod test_subsetting_quality;

use clap::{Parser, Subcommand};
use std::{io, path::PathBuf};

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    /// The subcommand to invoke.
    #[command(subcommand)]
    command: Commands,

    /// Whether to enable verbose output
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    GenerateLegacyGfsubsets,

    /// Downloads about 500GiB of common crawl data and processes it into bitsets of characters
    /// present in each crawled page.
    ///
    /// This must be called before any of the commands in this crate for generating subset data.
    ///
    /// This is **NOT** required for anything a user may want to do normally, and is only useful
    /// for developing mkwebfont typically.
    ///
    /// This takes about 500GiB of disk space (as it caches Common Crawl data to disk) and an
    /// extremely large amount of memory (to the order of 100-150GB) as it stores the generated
    /// bitset data uncompressed in memory while waiting to compress it.
    ///
    /// The Common Crawl raw dumps are not required for any other commands. They may be deleted
    /// after `common-crawl_bitsets-training` and `common-crawl_bitsets-validation` are generated.
    /// These files should total to about 7 GiB.
    DownloadCommonCrawl,

    /// Splits the monolithic training files for common crawl. This helps later steps.
    ///
    /// This takes about 7 GiB of disk, and an extremely large amount of memory.
    ///
    /// Requires that `download-common-crawl` is run first.
    SplitCommonCrawl,

    /// Tests the final download size of a given set of fonts on a set of website data.
    ///
    /// Requires that `download-common-crawl` is run first.
    TestSubsettingQuality(FileArgs),

    /// Generates the raw adjacency tables from common crawl data.
    ///
    /// This takes an extremely large amount of memory (to the order of 100-150GB) as it stores the
    /// bitset data uncompressed in memory in addition to multiple instances of large tables for
    /// the purpose of multithreading.
    ///
    /// Requires that `split-common-crawl` is run first.
    GenerateRawAdjacency,

    TestSubsetting(FileArgs),
}

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct FileArgs {
    files: Vec<PathBuf>,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let filter = if args.verbose {
        "debug,h2=info,hyper_util=info,reqwest=info,rustls=info"
    } else {
        "info"
    };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(io::stderr)
        .init();

    match args.command {
        Commands::GenerateLegacyGfsubsets => legacy_gfsubsets::main().await,
        Commands::DownloadCommonCrawl => download_common_crawl::download_common_crawl()
            .await
            .unwrap(),
        Commands::TestSubsettingQuality(path) => {
            test_subsetting_quality::test_subsetting_quality(&path.files)
                .await
                .unwrap()
        }
        Commands::GenerateRawAdjacency => generate_raw_adjacency::generate_raw_adjacency()
            .await
            .unwrap(),
        Commands::TestSubsetting(path) => test_subsetting::test_subsetting(&path.files).unwrap(),
        Commands::SplitCommonCrawl => split_common_crawl::split_common_crawl().await.unwrap(),
    }
}
