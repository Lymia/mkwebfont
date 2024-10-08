use anyhow::Result;
use clap::Parser;
use mkwebfont::{LoadedFontSetBuilder, SplitterPlan, Webroot};
use mkwebfont_common::FILTER_SPEC;
use std::{fs::OpenOptions, io, io::Write as IoWrite, path::PathBuf};
use tokio::runtime::Builder;
use tracing::{error, info, warn};

/// Generates webfonts for a given font.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// The font files to generate webfonts from.
    fonts: Vec<PathBuf>,

    /// The location to store generated .woff2 files in.
    #[arg(short = 's', long)]
    store: Option<PathBuf>,

    /// The URI at which the .woof2 store can be accessed at.
    #[arg(short = 'u', long)]
    store_uri: Option<String>,

    /// The path to write the .css file to, replacing the existing contents.
    #[arg(short = 'o', long)]
    output: Option<PathBuf>,

    /// The path to append the .css file to, adding it to the end of the file.
    #[arg(short = 'a', long)]
    append: Option<PathBuf>,

    /// Whether to enable verbose output
    #[arg(short = 'v', long)]
    verbose: bool,

    /// Include only certain font families.
    ///
    /// This is useful when working with TrueType Font Collections.
    #[arg(short = 'I', long)]
    include: Vec<String>,

    /// Exclude certain font families.
    ///
    /// This is useful when working with TrueType Font Collections.
    #[arg(short = 'E', long)]
    exclude: Vec<String>,

    /// Explicitly sets the splitting algorithm used.
    #[arg(long)]
    splitter: Option<SplitterImpl>,

    /// Automatically downloads a font family by name from Google Fonts.
    #[arg(short = 'f', long)]
    gfont: Vec<String>,

    /// The webroot to automatically generate webfonts for.
    ///
    /// This automatically generates `--subset-data`, `--gfont` and `--store-uri` arguments based
    /// on the contents of the webroot.
    #[arg(short = 'r', long)]
    webroot: Option<PathBuf>,

    /// Rewrites the contents at the webroot to use the webfonts.
    #[arg(short = 'w', long)]
    write_to_webroot: bool,

    /// Enables subsetting the input fonts before splitting them.
    #[arg(long)]
    subset: bool,

    /// Specifies how to subset fonts when `--subset` is enabled. The following directives are
    /// allowed:
    ///
    /// * `@<file path>` - Parses the file at a given path as a new-line seperated list of subset
    ///   directives.
    ///
    /// * `<font list>:<text data>` - Specifies that a given font stack is used with the given text
    ///   data.
    ///
    /// * `exclude:<font list>:<text data>` - Specifies that all characters in the given text data
    ///   are to never be included in any fonts in the given text list. This is meant for purposes
    ///   like using a font only for Chinese characters and not Latin characters.
    ///
    /// * `preload:<font list>:<text data>` - Specifies that all characters in the given text data
    ///   are to be included among the latin characters (or other split subset of the most common
    ///   characters)
    ///
    /// A font list is a comma-delimited list of font names.
    ///
    /// Text data may be `@<file path>` to load data from a given file, `#<unicode ranges>` for a
    /// list of unicode ranges in the same format as `unicode-range` in CSS, or raw string data
    /// that will be directly interpreted as text.
    #[arg(long)]
    subset_data: Vec<String>,

    /// Dumps all loaded fonts into a directory and return JSON data representing the paths.
    #[arg(long)]
    dump_fonts: Option<PathBuf>,
}

#[derive(clap::ValueEnum, Clone, Debug)]
enum SplitterImpl {
    Default,
    None,
    Gfonts,
}

async fn main_impl(args: Args) -> Result<()> {
    // check arguments
    if args.append.is_some() && args.output.is_some() {
        error!("Only one of `--append` and `--output` may be used in one invocation.");
        std::process::exit(1)
    }
    if args.store.is_none() && args.dump_fonts.is_none() {
        error!("`--store <STORE>` parameter must be provided.");
        std::process::exit(1)
    }
    if !args.exclude.is_empty() && !args.include.is_empty() {
        warn!("Only one of `--family` and `--exclude` may be used in one invocation.");
        std::process::exit(1)
    }
    if args.fonts.is_empty() && args.gfont.is_empty() && args.webroot.is_none() {
        warn!("No fonts sources were specified! An empty .css file will be generated.");
    }

    // prepare webfont generation context
    let mut ctx = SplitterPlan::new();
    if !args.exclude.is_empty() {
        ctx.blacklist_fonts(&args.exclude);
    }
    if !args.exclude.is_empty() {
        ctx.whitelist_fonts(&args.include);
    }
    match args.splitter {
        Some(SplitterImpl::None) => {
            ctx.no_splitter();
        }
        Some(SplitterImpl::Gfonts) => {
            ctx.gfonts_splitter();
        }
        _ => {
            ctx.gfonts_splitter();
        }
    }
    if args.subset {
        ctx.subset();
    }
    for spec in args.subset_data {
        ctx.subset_spec(&spec);
    }

    // load webroot
    let webroot = match args.webroot {
        Some(root) => Some(Webroot::load(&root).await?),
        None => None,
    };

    // load fonts
    let mut fonts = LoadedFontSetBuilder::new();
    fonts = fonts.load_from_disk(&args.fonts);
    fonts = fonts.load_from_gfonts(&args.gfont);
    if let Some(root) = &webroot {
        fonts = fonts.add_from_webroot(&root);
    }

    // dump fonts pass
    if let Some(path) = args.dump_fonts {
        info!("Dumping fonts to disk...");
        let result = fonts.build().await?.dump_fonts(&path, &ctx.build())?;
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    // process webfonts
    let styles = mkwebfont::process_webfont(&ctx, &fonts.build().await?, webroot.as_ref()).await?;

    // write webfonts to store and render css
    let count: usize = styles.webfonts.iter().map(|x| x.subset_count()).sum();
    info!("Writing {count} files to store...");

    let store = args.store.unwrap();
    if !store.exists() {
        std::fs::create_dir_all(&store)?;
    }
    styles.write_webfonts(&store)?;

    // write webfonts to the webroot.
    let store_uri = if let Some(store_uri) = args.store_uri {
        Some(store_uri)
    } else {
        None
    };
    if args.write_to_webroot {
        if webroot.is_some() {
            styles.rewrite_webroot(&store, store_uri.as_ref()).await?;
        } else {
            warn!("`--write-to-webroot` specified with no webroot. Ignoring.");
        }
    }

    // write css to output
    if let Some(target) = args.output {
        info!("Writing CSS to '{}'...", target.display());
        let css = styles.produce_css(&store, store_uri.as_ref())?;
        std::fs::write(target, css)?;
    } else if let Some(target) = args.append {
        info!("Appending CSS to '{}'...", target.display());
        let mut file = OpenOptions::new().write(true).append(true).open(target)?;
        let css = styles.produce_css(&store, store_uri.as_ref())?;
        file.write_all(css.as_bytes())?
    } else if !webroot.is_some() || !args.write_to_webroot {
        let css = styles.produce_css(&store, store_uri.as_ref())?;
        println!("{}", css);
    }

    // finalize
    info!("Done!");
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_env_filter(if args.verbose { FILTER_SPEC } else { "info" })
        .with_writer(io::stderr)
        .init();

    let rt = Builder::new_multi_thread().build()?;
    rt.block_on(main_impl(args))
}
