mod collect_data;
mod common_crawl_download;
mod common_crawl_split;
mod generate_adjacency_table;
mod generate_gfsubsets;
mod generate_glyphsets;
mod generate_validation_data;
mod test_subsetting;
mod test_subsetting_quality;

use async_recursion::async_recursion;
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
    /// The Common Crawl raw data is not required for any other commands. They may be safely
    /// deleted after this command completes.
    CommonCrawlDownload,

    /// Splits the monolithic training files for common crawl. This helps later steps.
    ///
    /// This takes about 7 GiB of disk, and an extremely large amount of memory.
    ///
    /// Requires that `common-crawl-download` is run first.
    CommonCrawlSplit,

    /// Generates the raw adjacency tables from split common crawl data.
    ///
    /// This takes an large amount of memory (to the order of 40-60GB) as it stores the bitset
    /// data uncompressed in memory in addition to multiple instances of large tables for the
    /// purpose of multithreading.
    ///
    /// Requires that `common-crawl-split` is run first.
    GenerateAdjacencyTable,

    /// Generates the Google Font subsets tables.
    ///
    /// Requires a Google Fonts API key in the `WEBFONT_APIKEY` environment variable.
    GenerateGfsubsets,

    /// Generates the basic glyph subsets from Google's glyph sets data.
    GenerateGlyphsets,

    /// Generates the validation data used to check the average size usage of a font.
    GenerateValidationData,

    /// Generates the final data package.
    ///
    /// Requires that `generate-adjacency-table` and `generate-gfsubsets` are run first.
    CollectData,

    /// Generates the final data package and runs all required steps before it.
    RunAll,

    TestSubsetting(FileArgs),

    /// Tests the final download size of a given set of fonts on a set of website data.
    ///
    /// Requires that `download-common-crawl` is run first.
    TestSubsettingQuality(FileArgs),

    /// Hashes the data in a package, to allow downloads to work correctly.
    HashPackage(FileArgs),
}

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct FileArgs {
    files: Vec<PathBuf>,
}

#[async_recursion]
async fn run(command: Commands) {
    match command {
        Commands::CommonCrawlDownload => common_crawl_download::download_common_crawl()
            .await
            .unwrap(),
        Commands::CommonCrawlSplit => common_crawl_split::split_common_crawl().await.unwrap(),
        Commands::GenerateAdjacencyTable => generate_adjacency_table::generate_raw_adjacency()
            .await
            .unwrap(),
        Commands::GenerateGfsubsets => generate_gfsubsets::main().await,
        Commands::GenerateGlyphsets => generate_glyphsets::generate_glyphsets().await.unwrap(),
        Commands::GenerateValidationData => generate_validation_data::generate_validation_data()
            .await
            .unwrap(),
        Commands::CollectData => collect_data::generate_data().await.unwrap(),
        Commands::TestSubsettingQuality(path) => {
            test_subsetting_quality::test_subsetting_quality(&path.files)
                .await
                .unwrap()
        }
        Commands::TestSubsetting(path) => test_subsetting::test_subsetting(&path.files).unwrap(),
        Commands::RunAll => {
            run(Commands::CommonCrawlDownload).await;
            run(Commands::CommonCrawlSplit).await;
            run(Commands::GenerateAdjacencyTable).await;
            run(Commands::GenerateGfsubsets).await;
            run(Commands::GenerateGlyphsets).await;
            run(Commands::CollectData).await;
        }
        Commands::HashPackage(_) => {}
    }
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
    run(args.command).await
}
