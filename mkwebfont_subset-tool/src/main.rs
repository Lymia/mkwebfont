mod common_crawl_download;
mod common_crawl_split;
mod generate_adjacency_table;
mod generate_data;
mod generate_gfsubsets;
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
    CommonCrawlDownload,

    /// Splits the monolithic training files for common crawl. This helps later steps.
    ///
    /// This takes about 7 GiB of disk, and an extremely large amount of memory.
    ///
    /// Requires that `download-common-crawl` is run first.
    CommonCrawlSplit,

    /// Generates the raw adjacency tables from split common crawl data.
    ///
    /// This takes an large amount of memory (to the order of 40-60GB) as it stores the bitset
    /// data uncompressed in memory in addition to multiple instances of large tables for the
    /// purpose of multithreading.
    ///
    /// Requires that `split-common-crawl` is run first.
    GenerateAdjacencyTable,

    /// Generates the Google Font subsets tables.
    ///
    /// Requires a Google Fonts API key in the `WEBFONT_APIKEY` environment variable.
    GenerateGfsubsets,

    /// Generates the final data package.
    ///
    /// Requires that `generate-adjacency-table` and `generate-gfsubsets` are run first.
    GenerateData,

    TestSubsetting(FileArgs),

    /// Tests the final download size of a given set of fonts on a set of website data.
    ///
    /// Requires that `download-common-crawl` is run first.
    TestSubsettingQuality(FileArgs),
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
        Commands::CommonCrawlDownload => common_crawl_download::download_common_crawl()
            .await
            .unwrap(),
        Commands::CommonCrawlSplit => common_crawl_split::split_common_crawl().await.unwrap(),
        Commands::GenerateAdjacencyTable => generate_adjacency_table::generate_raw_adjacency()
            .await
            .unwrap(),
        Commands::GenerateGfsubsets => generate_gfsubsets::main().await,
        Commands::GenerateData => generate_data::generate_data().unwrap(),
        Commands::TestSubsettingQuality(path) => {
            test_subsetting_quality::test_subsetting_quality(&path.files)
                .await
                .unwrap()
        }
        Commands::TestSubsetting(path) => test_subsetting::test_subsetting(&path.files).unwrap(),
    }
}
