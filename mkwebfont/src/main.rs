use clap::Parser;
use std::path::PathBuf;

/// Generates webfonts for a given font.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// The font file or .toml spec to generate webfonts from
    font: PathBuf,

    /// Whether to enable verbose output
    #[arg(short, long)]
    verbose: bool,
}

fn main() {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_env_filter(if args.verbose { "mkwebfont=debug,info" } else { "info" })
        .init();
    let data = mkwebfont::split_webfont(args.font).unwrap();
    println!("{}", data.render_css(""));
}
