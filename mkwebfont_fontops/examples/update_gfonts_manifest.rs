use anyhow::Result;
use chrono::DateTime;
use git2::Repository;
use glob::glob;
use mkwebfont_common::{
    compression::zstd_compress, download_cache::DownloadInfo, join_set::JoinSet,
};
use mkwebfont_fontops::{
    font_info::{AxisName, FontFaceWrapper},
    gfonts::gfonts_list::{GfontInfo, GfontStyleInfo, GfontsList},
};
use std::{collections::HashMap, io, path::PathBuf};
use tracing::{error, info};

// TODO: https://dl.rimin.moe/paste/lymia/NotoSerifDivesAkuru-_0hqwpxg8zdnbzn3k927znd1h3pa7pqws434mhnhazqighd89aicc.ttf

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(io::stderr)
        .init();

    let Some(repo_path) = std::env::args().skip(1).next() else {
        error!("Pass the path of a checkout of the `google/fonts` repository as the argument.");
        return Ok(());
    };

    let repository = Repository::open(&repo_path)?;
    let head = repository.head()?.target().unwrap();
    let rev = head.to_string();
    let rev_date = repository.find_commit(head)?.time();
    let rev_date = DateTime::from_timestamp(rev_date.seconds(), 0).unwrap();

    let cur_path = std::env::current_dir()?;
    std::env::set_current_dir(&repo_path)?;
    let mut fonts = Vec::new();
    for file in glob("apache/**/*.ttf")?
        .chain(glob("ofl/**/*.ttf")?)
        .chain(glob("ufl/**/*.ttf")?)
    {
        fonts.push(file?);
    }
    fonts.sort();
    let fonts_len = fonts.len();
    std::env::set_current_dir(cur_path)?;

    let mut joins = JoinSet::new();
    for font in fonts {
        let mut full_path = PathBuf::from(&repo_path);
        full_path.push(&font);

        joins.spawn(async move {
            let loaded_fonts = FontFaceWrapper::load(None, std::fs::read(&full_path)?)?;
            Ok(loaded_fonts
                .into_iter()
                .map(|x| (full_path.clone(), font.clone(), x))
                .collect())
        });
    }
    let font_faces = joins.join_vec().await?;
    let font_faces_len = font_faces.len();

    let mut font_info = HashMap::new();
    for (full_path, font_path, font) in font_faces {
        let url = format!("https://github.com/google/fonts/raw/{rev}/{}", font_path.display());

        let info = font_info
            .entry(font.font_family().to_string())
            .or_insert_with(|| GfontInfo {
                name: font.font_family().to_lowercase(),
                styles: vec![],
            });
        info.styles.push(GfontStyleInfo {
            style: font.parsed_font_style(),
            weight: if let Some(axis) = font
                .variations()
                .iter()
                .find(|x| x.axis == Some(AxisName::Weight))
            {
                *axis.range.start() as u32..=*axis.range.end() as u32
            } else {
                let weight = font.parsed_font_weight().as_num();
                weight..=weight
            },
            info: DownloadInfo::for_file(&full_path, &url)?,
        });
    }
    let mut font_info: Vec<_> = font_info.into_values().collect();
    font_info.sort_by(|a, b| a.name.cmp(&b.name));
    let font_info = GfontsList {
        repo_revision: rev.clone(),
        repo_date: rev_date.to_string(),
        repo_short_date: rev_date.format("%Y-%m-%d").to_string(),
        fonts: font_info,
    };
    std::fs::write(
        "mkwebfont_fontops/src/gfonts/gfonts_list.bin.zst",
        zstd_compress(&bincode::encode_to_vec(&font_info, bincode::config::standard())?)?,
    )?;
    info!("Revision: {rev}");
    info!("Revision Date: {rev_date}");
    info!("Found {fonts_len} font files, and {font_faces_len} font faces.");

    Ok(())
}
