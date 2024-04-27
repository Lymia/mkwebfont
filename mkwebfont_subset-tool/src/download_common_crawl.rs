//! We use a mix of historic and modern data to ensure that we have data from before SEO and AI
//! encrapified the internet.
//!
//! For our 'training' dataset, we have the following numbers:
//!  * 1000 archives from 2024.
//!  * 200 archives from 2019.
//!  * 200 archives from 2018.
//!  * 200 archives from 2017.
//!  * 200 archives from 2016.
//!  * 200 archives from 2015.
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
    "https://data.commoncrawl.org/crawl-data/CC-MAIN-2024-10/wet.paths.gz",
];

const TRAINING_FILES_LIST: &str = include_str!("cc-training-files.txt");
const VALIDATION_FILES_LIST: &str = include_str!("cc-validation-files.txt");

async fn find_download_links(list: &str) -> Result<Vec<String>> {
    let mut uris = HashMap::new();
    for path in PATH_URLS {
        let gz_data = reqwest::get(*path).await?.bytes().await?.to_vec();
        let mut data = Vec::new();
        GzDecoder::new(Cursor::new(gz_data)).read_to_end(&mut data)?;
        let data = String::from_utf8(data)?;

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
    for uri in links {
        let name = uri.split('/').last().unwrap();
        let target: PathBuf = format!("run/{target}/{name}").into();

        debug!("{uri} -> {}", target.display());

        if !target.exists() {
            let data = reqwest::get(uri).await?.bytes().await?.to_vec();
            fs::write(target, data)?;
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
            let warc = WarcReader::from_path_gzip(path)?;
            let mut builder = BitsetListBuilder::new();
            for record in warc.iter_records() {
                let record = record?;
                let str = std::str::from_utf8(record.body())?;
                builder.push_sample(str, |x| {
                    let category = GeneralCategory::of(x);
                    !category.is_other() && !category.is_separator()
                });
            }
            Ok(builder)
        })
    }

    let list = mkwebfont_common::bitset_list::build(joins.join().await?);
    let mut encoder = DataPackageEncoder::new(target);
    list.serialize("bitset_list", &mut encoder)?;
    Ok(encoder.build())
}

pub async fn download_common_crawl() -> Result<()> {
    download_uri_list("common-crawl", TRAINING_FILES_LIST).await?;
    download_uri_list("common-crawl-validation", VALIDATION_FILES_LIST).await?;

    let bs_train = tokio::spawn(process_list_to_bitmaps("common-crawl", TRAINING_FILES_LIST));
    let bs_valid =
        tokio::spawn(process_list_to_bitmaps("common-crawl-validation", VALIDATION_FILES_LIST));

    bs_train.await??.save("run/common-crawl_bitsets-training")?;
    bs_valid.await??.save("run/common-crawl_bitsets-validation")?;

    Ok(())
}
