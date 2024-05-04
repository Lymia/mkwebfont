use anyhow::Result;
use clap::Parser;
use mkwebfont::{LoadedFont, SubsetPlan};
use std::{
    collections::HashSet, fmt::Write, fs::OpenOptions, io, io::Write as IoWrite, path::PathBuf,
};
use tokio::runtime::Builder;
use tracing::{debug, error, info, warn};

/// Generates webfonts for a given font.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// The font files to generate webfonts from.
    fonts: Vec<PathBuf>,

    /// The location to store generated .woff2 files in.
    #[arg(short, long)]
    store: Option<PathBuf>,

    /// The URI at which the .woof2 store can be accessed at.
    #[arg(short = 'u', long)]
    store_uri: Option<String>,

    /// The path to write the .css file to, replacing the existing contents.
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// The path to append the .css file to, adding it to the end of the file.
    #[arg(short, long)]
    append: Option<PathBuf>,

    /// Whether to enable verbose output
    #[arg(short, long)]
    verbose: bool,

    /// Include only certain font families.
    ///
    /// This is useful when working with TrueType Font Collections.
    #[arg(short, long)]
    family: Vec<String>,

    /// Exclude certain font families.
    ///
    /// This is useful when working with TrueType Font Collections.
    #[arg(short = 'E', long)]
    exclude: Vec<String>,

    /// Always include a list of codepoints in the first partition split off from the font
    /// (usually latin).
    ///
    /// This can be used to allow unusual characters used throughout a website to be immediately
    /// available, rather than requiring loading another .woff2 font.
    #[arg(long)]
    preload: Vec<String>,
}

async fn main_impl(args: Args) -> Result<()> {
    // check arguments
    if args.append.is_some() && args.output.is_some() {
        error!("Only one of `--append` and `--output` may be used in one invocation.");
        std::process::exit(1)
    }
    if args.store.is_none() {
        error!("`--store <STORE>` parameter must be provided.");
        std::process::exit(1)
    }
    if args.fonts.is_empty() {
        warn!("No fonts were specified! An empty .css file will be generated.");
    }
    if !args.exclude.is_empty() && !args.family.is_empty() {
        warn!("Only one of `--family` and `--exclude` may be used in one invocation.");
        std::process::exit(1)
    }

    // prepare webfont generation context
    let mut ctx = SubsetPlan::new();
    for str in args.preload {
        ctx.preload_chars(str.chars());
    }
    if !args.exclude.is_empty() {
        ctx.blacklist_fonts(&args.exclude);
    }
    if !args.exclude.is_empty() {
        ctx.whitelist_fonts(&args.family);
    }

    // load fonts
    let fonts = mkwebfont::load_fonts_from_disk(&args.fonts).await?;

    // process webfonts
    let styles = mkwebfont::process_webfont(&ctx, &fonts).await?;

    let store_uri = if let Some(store_uri) = args.store_uri {
        store_uri
    } else {
        String::new()
    };

    // write webfonts to store and render css
    let count: usize = styles.iter().map(|x| x.subset_count()).sum();
    info!("Writing {count} files to store...");

    let mut css = String::new();
    let store = args.store.unwrap();
    for style in styles {
        writeln!(css, "{}", style.render_css(&store_uri))?;
        style.write_to_store(&store)?;
    }

    // write css to output
    if let Some(target) = args.output {
        info!("Writing CSS to '{}'...", target.display());
        std::fs::write(target, css)?;
    } else if let Some(target) = args.append {
        info!("Appending CSS to '{}'...", target.display());
        let mut file = OpenOptions::new().write(true).append(true).open(target)?;
        file.write_all(css.as_bytes())?
    } else {
        println!("{}", css);
    }

    // finalize
    info!("Done!");
    Ok(())
}
fn main_sync(args: Args) -> Result<()> {
    #[cfg(feature = "download-data")]
    let rt = Builder::new_multi_thread().enable_io().build()?;

    #[cfg(not(feature = "download-data"))]
    let rt = Builder::new_multi_thread().build()?;

    rt.block_on(main_impl(args))
}

fn main() {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_env_filter(if args.verbose { "mkwebfont=debug,info" } else { "info" })
        .with_writer(io::stderr)
        .init();

    match main_sync(args) {
        Ok(()) => {}
        Err(e) => error!("Error encountered: {e}"),
    }
}
