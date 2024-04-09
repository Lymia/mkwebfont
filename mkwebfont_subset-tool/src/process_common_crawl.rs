use anyhow::Result;
use roaring::RoaringBitmap;
use std::{
    fs,
    fs::File,
    io::{Cursor, Write},
};
use tokio::task::JoinHandle;
use tracing::info;
use warc::WarcReader;
use zstd::Encoder;

pub async fn parse_common_crawl() -> Result<()> {
    let mut joins: Vec<JoinHandle<Result<Vec<u8>>>> = Vec::new();
    for file in fs::read_dir("run/common-crawl")? {
        let file = file?;

        joins.push(tokio::spawn(async move {
            info!("Processing {}...", file.path().display());
            let warc = WarcReader::from_path_gzip(file.path())?;

            let mut file = Cursor::new(Vec::<u8>::new());
            for record in warc.iter_records() {
                let record = record?;
                let str = std::str::from_utf8(record.body())?;

                let mut chars = RoaringBitmap::new();
                for ch in str.chars() {
                    chars.insert(ch as u32);
                }
                chars.serialize_into(&mut file)?;
            }

            Ok(file.into_inner())
        }));
    }

    let file = File::create("test.bin.gz")?;
    let mut zip = Encoder::new(file, 10)?;
    for join in joins {
        zip.write_all(&join.await??)?;
    }
    zip.finish()?;
    Ok(())
}
