use crate::split_common_crawl::{SECTION_COUNT, SECTION_DIR, SECTION_TABLE};
use anyhow::Result;
use mkwebfont_common::{
    adjacency_array::AdjacencyArrayBuilder,
    bitset_list::{BitsetList, BitsetSection},
    data_package::{DataPackage, DataPackageEncoder},
    join_set::JoinSet,
};
use roaring::RoaringBitmap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info};
use unic_ucd_block::Block;
use unic_ucd_category::GeneralCategory;

// TODO: Use JoinSet

pub const RAW_ADJACENCY_PATH: &str = "run/common_crawl-raw_adjacency";
const ADJACENCY_ARRAY_NAME: &str = "common_crawl-adjacency";

async fn push_to_table(adjacency: &mut AdjacencyArrayBuilder, section: &BitsetSection) {
    info!("Processing {} pages from {}...", section.len(), section.source());
    let mut tmp = Vec::new();
    for bitmap in section.iter() {
        if bitmap.len() > MAX_CHARACTERS {
            continue;
        }
        adjacency.push_vector(&bitmap, section.chars(), &mut tmp);
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
const MIN_COUNT: u8 = 5;

fn load_section(i: usize) -> Result<BitsetList> {
    let path = format!("{SECTION_DIR}/section_{i}");
    let data = DataPackage::load(path)?;
    BitsetList::deserialize(SECTION_TABLE, &data)
}

pub async fn generate_raw_adjacency() -> Result<()> {
    let mut all_glyphs = RoaringBitmap::new();
    let mut webpage_count = 0u64;
    let mut omitted_count = 0u64;
    let mut count: [u8; 0x110000] = [0; 0x110000];
    {
        let mut joins = JoinSet::new();
        for i in 0..SECTION_COUNT {
            joins.spawn(async move {
                let bitsets = load_section(i)?;

                let mut all_glyphs = RoaringBitmap::new();
                let mut webpage_count = 0u64;
                let mut omitted_count = 0u64;
                let mut count: [u8; 0x110000] = [0; 0x110000];

                for (bitmap, chars) in bitsets.iter() {
                    webpage_count += 1;
                    if bitmap.len() > MAX_CHARACTERS {
                        omitted_count += 1;
                        continue;
                    }

                    for ch in bitmap {
                        let ch = chars[ch as usize];
                        all_glyphs.insert(ch);
                        count[ch as usize] = count[ch as usize].saturating_add(1);
                    }
                    if webpage_count % 200000 == 0 {
                        debug!("Preprocessing bitmaps as of {webpage_count}...");
                    }
                }

                Ok((all_glyphs, webpage_count, omitted_count, count))
            })
        }
        for (all_glyphs_n, webpage_count_n, omitted_count_n, count_n) in joins.join()? {
            all_glyphs.extend(all_glyphs_n);
            webpage_count += webpage_count_n;
            omitted_count += omitted_count_n;
            for i in 0..0x110000 {
                count[i] += count_n[i];
            }
        }
    }

    let mut filtered_glyphs = RoaringBitmap::new();
    let mut ommitted_glyphs = 0;
    for glyph in &all_glyphs {
        let ch = char::from_u32(glyph).unwrap();
        let cat = GeneralCategory::of(ch);
        if !cat.is_other() && !cat.is_separator() {
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

    let mut graph = AdjacencyArrayBuilder::new(ADJACENCY_ARRAY_NAME, &filtered_glyphs);
    {
        let remaining: Vec<_> = (0..SECTION_COUNT).collect();
        let remaining = Arc::new(Mutex::new(remaining));

        let mut joins = JoinSet::new();
        for i in 0..16 {
            let remaining = remaining.clone();
            joins.spawn(async move {
                let mut target = AdjacencyArrayBuilder::new(ADJACENCY_ARRAY_NAME, &filtered_glyphs);
                loop {
                    let section = remaining.lock().await.pop();
                    if let Some(i) = section {
                        let chunk = load_section(i)?;
                        for section in chunk.take_sections() {
                            push_to_table(&mut target, &section).await;
                        }
                    } else {
                        break;
                    }
                }
                Ok(target)
            });
        }
        for section in joins.join().await? {
            graph.join(section);
        }
    }

    info!("Outputting raw adjacency data...");
    let mut package = DataPackageEncoder::new("raw_adjacency");
    let graph = graph.build(1.5, |ch| Block::of(ch).map(|x| x.name));
    graph.serialize("raw_adjacency", &mut package)?;
    let package = package.build();
    std::fs::write(RAW_ADJACENCY_PATH, package.encode()?)?;

    Ok(())
}
