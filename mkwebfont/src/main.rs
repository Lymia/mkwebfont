use clap::Parser;
use mkwebfont::SplitWebfontCtx;
use roaring::RoaringBitmap;
use std::{fmt::Write, io, path::PathBuf};
use tracing::{info, warn};

/// Generates webfonts for a given font.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// The font files to generate webfonts from.
    fonts: Vec<PathBuf>,

    /// The location to store generated woff2 files in.
    #[arg(short, long)]
    store: PathBuf,

    /// Where to write the generated .css file.
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Whether to enable verbose output
    #[arg(short, long)]
    verbose: bool,

    /// The URI to prepend to the generated .css file.
    #[arg(long)]
    store_uri: Option<String>,

    /// Always include a list of codepoints in the first partition split off from the font
    /// (usually latin). This can be used to allow unusual characters used throughout a website
    /// to be immediately available, rather than requiring loading another woff font.
    #[arg(long)]
    preload: Vec<String>,
}

fn main() {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_env_filter(if args.verbose { "mkwebfont=debug,info" } else { "info" })
        .with_writer(io::stderr)
        .init();

    if args.fonts.is_empty() {
        warn!("No fonts were specified! An empty .css file will be generated.");
    }

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
    split_ctx.preload_codepoints = preload_codepoints;
    for font in &args.fonts {
        info!("Processing webfont: {}", font.display());
        for data in mkwebfont::split_webfont(&split_ctx, font, &args.store).unwrap() {
            writeln!(css, "{}", data.render_css(&store_uri)).unwrap();
        }
    }

    if let Some(target) = args.output {
        std::fs::write(target, css).unwrap();
    } else {
        println!("{}", css);
    }
}
