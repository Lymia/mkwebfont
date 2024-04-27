//! We use a mix of historic and modern data to ensure that we have data from before SEO and AI
//! encrapified the internet.
//!
//! For our 'training' dataset, we have the following numbers:
//!  * 2000 archives from 2023-2024.
//!  * 400 archives from 2019.
//!  * 400 archives from 2018.
//!  * 400 archives from 2017.
//!  * 400 archives from 2016.
//!  * 400 archives from 2015.
//!
//! For validation, we have 10% of that amount from the same sources.
//!
//! For testing, uh. I'm just going to skip that, this isn't actually machine learning work.

use anyhow::Result;
use flate2::read::GzDecoder;
use mkwebfont_common::{
    bitset_list::BitsetListBuilder,
    data_package::{DataPackage, DataPackageEncoder},
    join_set::JoinSet,
};
use std::{
    collections::HashMap,
    fs,
    io::{Cursor, Read},
    path::PathBuf,
};
use tokio::task::JoinHandle;
use tracing::{debug, info};
use unic_ucd_category::GeneralCategory;
use warc::WarcReader;

const STORE_PREFIX: &str = "https://data.commoncrawl.org";

const PATH_URLS: &[&str] = &[
    "https://data.commoncrawl.org/crawl-data/CC-MAIN-2015-40/wet.paths.gz",
    "https://data.commoncrawl.org/crawl-data/CC-MAIN-2015-48/wet.paths.gz",
    "https://data.commoncrawl.org/crawl-data/CC-MAIN-2016-26/wet.paths.gz",
    "https://data.commoncrawl.org/crawl-data/CC-MAIN-2016-50/wet.paths.gz",
    "https://data.commoncrawl.org/crawl-data/CC-MAIN-2017-26/wet.paths.gz",
    "https://data.commoncrawl.org/crawl-data/CC-MAIN-2017-51/wet.paths.gz",
    "https://data.commoncrawl.org/crawl-data/CC-MAIN-2018-26/wet.paths.gz",
    "https://data.commoncrawl.org/crawl-data/CC-MAIN-2018-51/wet.paths.gz",
    "https://data.commoncrawl.org/crawl-data/CC-MAIN-2019-26/wet.paths.gz",
    "https://data.commoncrawl.org/crawl-data/CC-MAIN-2019-51/wet.paths.gz",
    "https://data.commoncrawl.org/crawl-data/CC-MAIN-2023-50/wet.paths.gz",
    "https://data.commoncrawl.org/crawl-data/CC-MAIN-2024-10/wet.paths.gz",
];

const TRAINING_FILES_LIST: &str = include_str!("cc-training-files.txt");
const VALIDATION_FILES_LIST: &str = include_str!("cc-validation-files.txt");

async fn find_download_links(list: &str) -> Result<Vec<String>> {
    fs::create_dir_all("run/common-crawl-indexes")?;

    let mut uris = HashMap::new();
    for path in PATH_URLS {
        let components: Vec<_> = path.split("/").collect();
        let target = components[components.len() - 2];
        let target: PathBuf =
            PathBuf::from(format!("run/common-crawl-indexes/{target}_wet.paths.txt"));

        if !target.exists() {
            debug!("Downloading index from {path}...");
            let gz_data = reqwest::get(*path).await?.bytes().await?.to_vec();
            let mut data = Vec::new();
            GzDecoder::new(Cursor::new(gz_data)).read_to_end(&mut data)?;
            let data = String::from_utf8(data)?;
            fs::write(&target, data)?;
        } else {
            debug!("Using cached index from {}...", target.display());
        }

        let data = fs::read_to_string(&target)?;
        for line in data.trim().split('\n') {
            uris.insert(
                line.split('/').last().unwrap().to_string(),
                format!("{STORE_PREFIX}/{line}"),
            );
        }
    }

    let mut links = Vec::new();
    for line in list.trim().split('\n') {
        links.push(
            uris.remove(line)
                .expect(&format!("expected file does not exist in lists: {line}")),
        );
    }
    Ok(links)
}

async fn download_uri_list(target: &str, list: &str) -> Result<()> {
    info!("Loading list for '{target}'...");

    let links = find_download_links(list).await?;
    fs::create_dir_all(format!("run/{target}"))?;

    let mut downloads: Vec<JoinHandle<Result<()>>> = Vec::new();
    for uri in links {
        let target = target.to_string();
        downloads.push(tokio::spawn(async move {
            let name = uri.split('/').last().unwrap();
            let target: PathBuf = format!("run/{target}/{name}").into();

            debug!("{uri} -> {}", target.display());

            if !target.exists() {
                let data = reqwest::get(uri).await?.bytes().await?.to_vec();
                fs::write(target, data)?;
            }

            Ok(())
        }));

        if downloads.len() == 5 {
            downloads.pop().unwrap().await??;
        }
    }
    Ok(())
}

pub async fn process_list_to_bitmaps(target: &str, list: &str) -> Result<DataPackage> {
    let mut joins = JoinSet::new();
    for file in list.trim().split('\n') {
        let path = PathBuf::from(format!("run/{target}/{file}"));
        assert!(path.exists());
        joins.spawn(async move {
            info!("Processing {}...", path.display());
            let warc = WarcReader::from_path_gzip(&path)?;
            let mut builder = BitsetListBuilder::new(&path.file_name().unwrap().to_string_lossy());
            builder.filter_chars(|x| {
                let category = GeneralCategory::of(x);
                !category.is_other() && !category.is_separator()
            });
            for record in warc.iter_records() {
                let record = record?;
                let str = std::str::from_utf8(record.body())?;
                builder.push_sample(str);
            }
            Ok(builder.optimize())
        })
    }

    let list = mkwebfont_common::bitset_list::build(joins.join().await?);
    let mut encoder = DataPackageEncoder::new(target);
    list.serialize("bitset_list", &mut encoder)?;
    Ok(encoder.build())
}

pub async fn download_common_crawl() -> Result<()> {
    let mut joins = JoinSet::new();
    joins.spawn(download_uri_list("common-crawl", TRAINING_FILES_LIST));
    joins.spawn(download_uri_list("common-crawl-validation", VALIDATION_FILES_LIST));
    joins.join().await?;

    let bs_train = tokio::spawn(process_list_to_bitmaps("common-crawl", TRAINING_FILES_LIST));
    let bs_valid =
        tokio::spawn(process_list_to_bitmaps("common-crawl-validation", VALIDATION_FILES_LIST));

    bs_train.await??.save("run/common-crawl_bitsets-training")?;
    bs_valid
        .await??
        .save("run/common-crawl_bitsets-validation")?;

    Ok(())
}
