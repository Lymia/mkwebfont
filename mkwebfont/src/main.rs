use anyhow::Result;
use clap::Parser;
use mkwebfont::{LoadedFontSetBuilder, SplitterPlan};
use mkwebfont_common::FILTER_SPEC;
use std::{fmt::Write, fs::OpenOptions, io, io::Write as IoWrite, path::PathBuf};
use tokio::runtime::Builder;
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

    /// Prints a report about how much network the font would use in common situations.
    #[arg(long)]
    print_report: bool,

    /// Explicitly sets the splitting algorithm used.
    #[arg(long)]
    splitter: Option<SplitterImpl>,

    /// Configures how to subset the fonts, or configures additional CSS code to generate. This
    /// can be any of the following statements.
    ///
    /// * `@<file name>`: A new-line delimited list of spec statements.
    ///
    /// * `fallback:<font list>`: Marks the list of fonts as being used for fallback, at the end
    ///   of every character not found in a font stack. You should generally use this with a high
    ///   quality font such as Noto Sans.
    ///
    /// * `preload:<font list>:<text data>`: Hints that certain characters occur on most pages, and
    ///   should be placed in the same split font as the primary script.
    ///
    /// * `subset:<font list>:<text data>`: Subsets a font stack with the given text data.
    ///
    /// * `stack:<name>[/<language>]:<font list>[:<text data>]`: Creates a font stack, possibly
    ///   subsetting it with specific text data. Generates a `.font-<name>` CSS class and a
    ///   `--font-<name>` CSS variable.
    ///
    /// * `exclusion:<font list>:<text data>`: Specifies that all characters in a given text data
    ///   are never to be included in the given list of fonts.
    ///
    /// * `whitelist`: Specifies that all fonts not explicitly subset are to be considered empty,
    ///   and not generated at all.
    ///
    /// * `munge_font_names`: Modifies font names such that they are more unique in the actual CSS.
    ///
    /// Font lists are comma seperated lists of fonts, or `*`. Text data is raw text data,
    /// `@<file name>` to load it from a file, or `*` to include all characters. Syntax bracketed
    /// in `[x]` is optional and may be omitted.
    #[arg(short = 'S', long)]
    spec: Vec<String>,
}

#[derive(clap::ValueEnum, Clone, Debug)]
enum SplitterImpl {
    Default,
    None,
    Gfonts,
    Adjacency,
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
    let mut ctx = SplitterPlan::new();
    if !args.exclude.is_empty() {
        ctx.blacklist_fonts(&args.exclude);
    }
    if !args.exclude.is_empty() {
        ctx.whitelist_fonts(&args.family);
    }
    if args.print_report {
        ctx.print_report();
    }
    match args.splitter {
        Some(SplitterImpl::None) => {
            ctx.no_splitter();
        }
        Some(SplitterImpl::Gfonts) => {
            ctx.gfonts_splitter();
        }
        Some(SplitterImpl::Adjacency) => {
            ctx.adjacency_splitter();
        }
        _ => {
            ctx.gfonts_splitter();
        }
    }
    /*for subset in args.subset {
        ctx.subset_chars(subset.chars());
    }
    for subset in args.subset_from {
        ctx.subset_chars(std::fs::read_to_string(subset)?.chars());
    }*/

    // load fonts
    let fonts = LoadedFontSetBuilder::load_from_disk(&args.fonts)
        .await?
        .build();

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
        .with_env_filter(if args.verbose { FILTER_SPEC } else { "info" })
        .with_writer(io::stderr)
        .init();

    match main_sync(args) {
        Ok(()) => {}
        Err(e) => error!("Error encountered: {e}"),
    }
}
