use anyhow::Result;
use mkwebfont_common::{adjacency_array::AdjacencyArrayBuilder, data_package::DataPackageEncoder};
use roaring::RoaringBitmap;
use std::{fs::File, io::BufReader, sync::Arc};
use tracing::{debug, info};
use unic_ucd_block::Block;
use unic_ucd_category::GeneralCategory;
use zstd::Decoder;

// TODO: Use JoinSet

pub const RAW_ADJACENCY_PATH: &str = "run/common_crawl-raw_adjacency";
const ADJACENCY_ARRAY_NAME: &str = "common_crawl-adjacency";

async fn push_to_table(
    i: usize,
    webpage_count: u64,
    adjacency: Arc<AdjacencyArrayBuilder>,
    bitmaps: Vec<RoaringBitmap>,
) {
    info!("Processing {} pages as of {i}/{webpage_count} bitmaps ...", bitmaps.len());
    let mut tmp = Vec::new();
    for bitmap in bitmaps {
        adjacency.push_vector(&bitmap, &mut tmp);
    }
}

/// A threshold character count after which we assume the page is a dictionary or some other
/// similar listing that does not reflect a typical webpage.
///
/// There is some bias against CJK here, but 1750 characters is still a *LOT* for even complicated
/// CJK pages. The longest Wikipedia pages I've found are in the 1000-1800 range, with the longest
/// pages on the wiki reaching into the 2500s.
///
/// This is probably okay, even for CJK, those pages give bad data for this use case.
const MAX_CHARACTERS: u64 = 1750;

/// The minimum number of samples for a character for it to not be excluded.
///
/// This prevents groupings based on extremely small numbers of websites.
const MIN_COUNT: u8 = 10;

pub async fn generate_raw_adjacency() -> Result<()> {
    let mut all_glyphs = RoaringBitmap::new();
    let mut webpage_count = 0u64;
    let mut omitted_count = 0u64;
    let mut count: [u8; 0x110000] = [0; 0x110000];
    {
        let path = File::open("run/common-crawl_parsed-bitmaps.zst")?;
        let reader = BufReader::new(path);
        let mut zstd = Decoder::new(reader)?;

        while let Ok(bitmap) = RoaringBitmap::deserialize_from(&mut zstd) {
            webpage_count += 1;
            if bitmap.len() > MAX_CHARACTERS {
                omitted_count += 1;
                continue;
            }

            for ch in bitmap {
                all_glyphs.insert(ch);
                count[ch as usize] = count[ch as usize].saturating_add(1);
            }
            if webpage_count % 200000 == 0 {
                debug!("Preprocessing bitmaps as of {webpage_count}...");
            }
        }
    }

    let mut filtered_glyphs = RoaringBitmap::new();
    let mut ommitted_glyphs = 0;
    for glyph in &all_glyphs {
        let ch = char::from_u32(glyph).unwrap();
        let cat = GeneralCategory::of(ch);
        if !cat.is_other() && !cat.is_separator() && cat != GeneralCategory::PrivateUse {
            if count[glyph as usize] >= MIN_COUNT {
                filtered_glyphs.insert(glyph);
            } else {
                ommitted_glyphs += 1;
            }
        }
    }

    info!("Codepoint count: {}", all_glyphs.len());
    info!("Webpage count: {webpage_count} ({omitted_count} omitted)");
    info!(
        "Filtered codepoint count: {} ({ommitted_glyphs} omitted)",
        filtered_glyphs.len()
    );

    let graph = Arc::new(AdjacencyArrayBuilder::new(ADJACENCY_ARRAY_NAME, &filtered_glyphs));
    {
        let path = File::open("run/common-crawl_parsed-bitmaps.zst")?;
        let reader = BufReader::new(path);
        let mut zstd = Decoder::new(reader)?;

        let mut i = 0;
        let mut bitmaps = Vec::new();
        let mut threads = Vec::new();
        while let Ok(bitmap) = RoaringBitmap::deserialize_unchecked_from(&mut zstd) {
            if bitmap.len() <= MAX_CHARACTERS {
                bitmaps.push(bitmap);
            }

            i += 1;
            if i % 200000 == 0 {
                debug!("Submitting bitmaps as of {i}/{webpage_count} for processing...");
                let task = tokio::spawn(push_to_table(
                    i,
                    webpage_count,
                    graph.clone(),
                    std::mem::replace(&mut bitmaps, Vec::new()),
                ));
                threads.push(task);
            }
        }
        push_to_table(i, webpage_count, graph.clone(), bitmaps).await;

        for thread in threads {
            thread.await?;
        }
    }

    info!("Outputting raw adjacency data...");
    let mut package = DataPackageEncoder::new("raw_adjacency");
    let graph = graph.build(150, 1.6, |ch| Block::of(ch).map(|x| x.name));
    graph.serialize("raw_adjacency", &mut package)?;
    let package = package.build();
    std::fs::write(RAW_ADJACENCY_PATH, package.encode()?)?;

    Ok(())
}
