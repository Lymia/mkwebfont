use anyhow::Result;
use flate2::read::GzDecoder;
use std::{
    fmt::Write,
    io::{Cursor, Read},
    path::PathBuf,
};
use tracing::info;

const PATH_URLS: &[&str] = &[
    "https://data.commoncrawl.org/crawl-data/CC-MAIN-2024-10/wet.paths.gz",
    "https://data.commoncrawl.org/crawl-data/CC-MAIN-2023-50/wet.paths.gz",
    "https://data.commoncrawl.org/crawl-data/CC-MAIN-2022-49/wet.paths.gz",
];
const STORE_PREFIX: &str = "https://data.commoncrawl.org";
const DOWNLOAD_GB: usize = 65;

pub async fn download_common_crawl() -> Result<()> {
    let mut url_list = String::new();
    for path in PATH_URLS {
        let gz_data = reqwest::get(*path).await?.bytes().await?.to_vec();
        let mut data = Vec::new();
        GzDecoder::new(Cursor::new(gz_data)).read_to_end(&mut data)?;
        let data = String::from_utf8(data)?;
        writeln!(url_list, "{}", data.trim())?;
    }

    std::fs::create_dir_all("run/common-crawl")?;
    for line in url_list.split('\n').take(DOWNLOAD_GB * 10) {
        let name = line.split('/').last().unwrap();
        let uri = format!("{STORE_PREFIX}/{line}");
        let target: PathBuf = format!("run/common-crawl/{name}").into();

        info!("{uri} -> {}", target.display());

        if !target.exists() {
            let data = reqwest::get(uri).await?.bytes().await?.to_vec();
            std::fs::write(target, data)?;
        }
    }

    Ok(())
}
