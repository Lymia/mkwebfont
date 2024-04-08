use anyhow::*;
use clap::Parser;
use mkwebfont::{LoadedFont, WebfontCtxBuilder};
use std::{fmt::Write, fs::OpenOptions, io, io::Write as IoWrite, path::PathBuf};
use std::collections::HashSet;
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

    /// Functions like preload, but allows preloading only for a specific font.
    ///
    /// The format is: `--preload-in "Font Family Name:abcdef"`
    #[arg(long)]
    preload_in: Vec<String>,

    /// Uses the subset manifest file at the given path.
    ///
    /// This can be used to customize which characters are subsetted into which groups.
    #[arg(long)]
    subset_manifest: Option<PathBuf>,

    /// Writes the default subset manifest file to the given path.
    #[arg(long)]
    write_default_subset_manifest: Option<PathBuf>,

    /// Uses the splitter tuning file at the given path.
    ///
    /// This can be used to customize how mkwebfont decides which subsets to apply to a given font.
    /// You will likely not need to use this.
    #[arg(long)]
    splitter_tuning: Option<PathBuf>,

    /// Writes the default splitter tuning file to the given path.
    #[arg(long)]
    write_default_splitter_tuning: Option<PathBuf>,
}

fn main_impl(args: Args) -> Result<()> {
    // write default configuration
    {
        let mut early_exit = false;
        if let Some(path) = args.write_default_subset_manifest {
            info!("Writting default subset manifest to {}", path.display());
            std::fs::write(path, include_str!("subset_manifest_default.toml"))?;
            early_exit = true;
        }
        if let Some(path) = args.write_default_splitter_tuning {
            info!("Writting default splitter configuration to {}", path.display());
            std::fs::write(path, include_str!("splitter_default_tuning.toml"))?;
            early_exit = true;
        }
        if early_exit {
            return Ok(());
        }
    }

    // check arguments
    if args.append.is_some() && args.output.is_some() {
        error!("`--append` and `--output` parameter cannot be used together.");
        std::process::exit(1)
    }
    if args.store.is_none() {
        error!("`--store <STORE>` parameter must be provided.");
        std::process::exit(1)
    }
    if args.fonts.is_empty() {
        warn!("No fonts were specified! An empty .css file will be generated.");
    }

    // prepare webfont generation context
    let mut ctx = WebfontCtxBuilder::new(&args.store.unwrap());
    for str in args.preload {
        ctx.preload(str.chars());
    }
    if let Some(manifest) = args.subset_manifest {
        ctx.add_subset_manifest(&std::fs::read_to_string(manifest)?);
    }
    if let Some(tuning) = args.splitter_tuning {
        ctx.add_splitter_tuning(&std::fs::read_to_string(tuning)?);
    }
    let ctx = ctx.build()?;

    // load fonts
    let mut font_data = Vec::new();
    for font in &args.fonts {
        font_data.push((font, std::fs::read(font)?));
    }
    let mut raw_fonts = Vec::new();
    for (path, font) in &font_data {
        info!("Loading fonts from path: {}", path.display());
        raw_fonts.extend(LoadedFont::load(font.as_slice())?);
    }
    info!("Found {} fonts:", raw_fonts.len());
    let mut accepted_fonts = Vec::new();
    {
        let exclude: HashSet<_> = args.exclude.into_iter().collect();
        let family: HashSet<_> = args.family.into_iter().collect();
        for font in raw_fonts {
            let name = font.font_family();
            let style = font.font_style();
            let is_excluded = exclude.contains(name);
            let is_not_whitelisted = !family.is_empty() && !family.contains(name);

            if is_excluded {
                info!(" - {name} {style} (excluded)");
            } else if is_not_whitelisted {
                info!(" - {name} {style} (not in whitelist)");
            } else {
                info!(" - {name} {style}");
                accepted_fonts.push(font);
            }
        }
    }

    // process webfonts
    let styles = mkwebfont::split_webfont(&ctx, &accepted_fonts)?;

    let store_uri = if let Some(store_uri) = args.store_uri {
        store_uri
    } else {
        String::new()
    };
    let mut css = String::new();
    for style in styles {
        writeln!(css, "{}", style.render_css(&store_uri))?;
    }

    // write css to output
    if let Some(target) = args.output {
        std::fs::write(target, css)?;
    } else if let Some(target) = args.append {
        let mut file = OpenOptions::new().write(true).append(true).open(target)?;
        file.write_all(css.as_bytes())?
    } else {
        println!("{}", css);
    }

    Ok(())
}

fn main() {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_env_filter(if args.verbose { "mkwebfont=debug,info" } else { "info" })
        .with_writer(io::stderr)
        .init();

    match main_impl(args) {
        Result::Ok(()) => {}
        Result::Err(e) => error!("Error encountered: {e}"),
    }
}
