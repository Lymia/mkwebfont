use clap::Parser;
use mkwebfont::SplitWebfontCtx;
use roaring::RoaringBitmap;
use std::{fmt::Write, io, path::PathBuf};
use tracing::{error, info, warn};

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
    #[arg(long)]
    store_uri: Option<String>,

    /// Where to write the generated .css file.
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Whether to enable verbose output
    #[arg(short, long)]
    verbose: bool,

    /// Always include a list of codepoints in the first partition split off from the font
    /// (usually latin).
    ///
    /// This can be used to allow unusual characters used throughout a website to be immediately
    /// available, rather than requiring loading another .woff2 font.
    #[arg(long)]
    preload: Vec<String>,

    /// Uses the splitter tuning file at the given path.
    #[arg(long)]
    splitter_tuning: Option<PathBuf>,

    /// Writes the default splitter tuning file to the given path.
    #[arg(long)]
    write_default_splitter_tuning: Option<PathBuf>,
}

fn main() {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_env_filter(if args.verbose { "mkwebfont=debug,info" } else { "info" })
        .with_writer(io::stderr)
        .init();

    // write splitter configuration
    if let Some(path) = args.write_default_splitter_tuning {
        info!("Writting default splitter configuration to {}", path.display());
        std::fs::write(path, include_str!("splitter_default_tuning.toml")).unwrap();
        return;
    }

    // check arguments
    if args.store.is_none() {
        error!("`--store <STORE>` parameter must be provided.");
        std::process::exit(1)
    }
    if args.fonts.is_empty() {
        warn!("No fonts were specified! An empty .css file will be generated.");
    }

    // do actual webfont generaetion
    let mut css = String::new();
    let store_uri = if let Some(store_uri) = args.store_uri {
        store_uri
    } else {
        String::new()
    };

    let mut preload_codepoints = RoaringBitmap::new();
    for str in args.preload {
        for char in str.chars() {
            preload_codepoints.insert(char as u32);
        }
    }

    let mut split_ctx = SplitWebfontCtx::default();
    if let Some(tuning) = args.splitter_tuning {
        split_ctx.splitter_tuning = Some(std::fs::read_to_string(tuning).unwrap());
    }
    split_ctx.preload_codepoints = preload_codepoints;
    for font in &args.fonts {
        info!("Processing webfont: {}", font.display());
        for data in
            mkwebfont::split_webfont(&split_ctx, font, args.store.as_ref().unwrap()).unwrap()
        {
            writeln!(css, "{}", data.render_css(&store_uri)).unwrap();
        }
    }

    if let Some(target) = args.output {
        std::fs::write(target, css).unwrap();
    } else {
        println!("{}", css);
    }
}
