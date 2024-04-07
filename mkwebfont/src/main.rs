use clap::Parser;
use std::{fmt::Write, io, path::PathBuf};
use tracing::info;

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
}

fn main() {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_env_filter(if args.verbose { "mkwebfont=debug,info" } else { "info" })
        .with_writer(io::stderr)
        .init();

    let mut css = String::new();
    let store_uri = if let Some(store_uri) = args.store_uri {
        store_uri
    } else {
        String::new()
    };
    for font in &args.fonts {
        info!("Processing webfont: {}", font.display());
        let data = mkwebfont::split_webfont(font, &args.store).unwrap();
        writeln!(css, "{}", data.render_css(&store_uri)).unwrap();
    }

    if let Some(target) = args.output {
        std::fs::write(target, css).unwrap();
    } else {
        println!("{}", css);
    }
}
