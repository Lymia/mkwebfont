use anyhow::Result;
use roaring::RoaringBitmap;
use std::{fs::File, io::BufReader};
use zstd::Decoder;

pub fn test_subsetting_quality() -> Result<()> {
    let reader = BufReader::new(File::open("run/common-crawl-validation_parsed-bitmaps.zst")?);
    let mut zstd = Decoder::new(reader)?;

    let mut count = 0;
    while let Ok(bitmap) = RoaringBitmap::deserialize_from(&mut zstd) {
        count += 1;
    }
    println!("{count} bitmaps");

    Ok(())
}
