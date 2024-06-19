use anyhow::Result;
use git2::Repository;
use glob::glob;
use mkwebfont_common::FILTER_SPEC;
use std::io;
use tracing::{error, info};

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(FILTER_SPEC)
        .with_writer(io::stderr)
        .init();

    let Some(path) = std::env::args().skip(1).next() else {
        error!("Pass the path of a checkout of the `google/fonts` repository as the argument.");
        return Ok(());
    };

    let repository = Repository::open(&path)?;
    let rev = repository.head()?.target().unwrap().to_string();
    info!("Revision: {rev}");

    let cur_path = std::env::current_dir()?;
    std::env::set_current_dir(&path)?;
    let mut fonts = Vec::new();
    for file in glob("apache/**/*.ttf")?
        .chain(glob("ofl/**/*.ttf")?)
        .chain(glob("ufl/**/*.ttf")?)
    {
        fonts.push(file?);
    }
    info!("Found {} font files.", fonts.len());
    std::env::set_current_dir(cur_path)?;

    Ok(())
}
