use crate::common_crawl_split::{SPLIT_SECTION_COUNT, SPLIT_SECTION_DIR, SPLIT_SECTION_TAG};
use anyhow::Result;
use mkwebfont_common::{
    join_set::JoinSet,
    model::{
        adjacency_array::AdjacencyArrayBuilder,
        bitset_list::{BitsetList, BitsetSection},
        data_package::{DataPackage, DataPackageEncoder},
    },
};
use roaring::RoaringBitmap;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::sync::Mutex;
use tracing::{debug, info, info_span};
use unicode_blocks::find_unicode_block;
use unicode_properties::{GeneralCategoryGroup, UnicodeGeneralCategory};

pub const ADJACENCY_PATH: &str = "run/common_crawl-adjacency";
pub const ADJACENCY_TAG: &str = "adjacency";
const ADJACENCY_VERSION: &str = "v0.1.0";

async fn push_to_table(
    adjacency: &mut AdjacencyArrayBuilder,
    section: &BitsetSection,
    i: &AtomicUsize,
    j: usize,
) {
    info!(
        "Processing {} pages from {}... ({}/{j})",
        section.len(),
        section.source(),
        i.fetch_add(1, Ordering::Relaxed),
    );
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
const MIN_COUNT: u8 = 50;

fn load_section(i: usize) -> Result<BitsetList> {
    let path = format!("{SPLIT_SECTION_DIR}/section_{i}");
    let mut data = DataPackage::load(path)?;
    BitsetList::deserialize(data.take_section(SPLIT_SECTION_TAG)?)
}

pub async fn generate_raw_adjacency() -> Result<()> {
    let mut all_glyphs = RoaringBitmap::new();
    let mut webpage_count = 0u64;
    let mut omitted_count = 0u64;
    let mut count = Box::new([0u8; 0x110000]);
    let mut sections = 0;
    {
        let mut joins = JoinSet::new();
        for i in 0..SPLIT_SECTION_COUNT {
            let span = info_span!("count", section = i);
            let _enter = span.enter();

            joins.spawn(async move {
                debug!("Begin count...");

                let bitsets = load_section(i)?;

                let mut all_glyphs = RoaringBitmap::new();
                let mut webpage_count = 0u64;
                let mut omitted_count = 0u64;
                let mut count = Box::new([0u8; 0x110000]);

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
                }

                debug!("End count...");
                Ok((all_glyphs, webpage_count, omitted_count, count, bitsets.sections().len()))
            })
        }
        for (all_glyphs_n, webpage_count_n, omitted_count_n, count_n, sections_n) in
            joins.join().await?
        {
            all_glyphs.extend(all_glyphs_n);
            webpage_count += webpage_count_n;
            omitted_count += omitted_count_n;
            for i in 0..0x110000 {
                count[i] += count_n[i];
            }
            sections += sections_n;
        }
    }

    let mut filtered_glyphs = RoaringBitmap::new();
    let mut ommitted_glyphs = 0;
    for glyph in &all_glyphs {
        let ch = char::from_u32(glyph).unwrap();
        let cat = ch.general_category_group();
        if cat != GeneralCategoryGroup::Other && cat != GeneralCategoryGroup::Separator {
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

    let mut graph;
    {
        let remaining: Vec<_> = (0..SPLIT_SECTION_COUNT).collect();
        let remaining = Arc::new(Mutex::new(remaining));

        let mut joins = JoinSet::new();
        let atomic = Arc::new(AtomicUsize::new(1));
        for i in 0..16 {
            let span = info_span!("build_array", thread = i);
            let _enter = span.enter();

            let filtered_glyphs = filtered_glyphs.clone();
            let remaining = remaining.clone();
            let atomic = atomic.clone();

            joins.spawn(async move {
                let mut target = AdjacencyArrayBuilder::new(&filtered_glyphs);
                loop {
                    let section = remaining.lock().await.pop();
                    if let Some(i) = section {
                        let chunk = load_section(i)?;
                        for section in chunk.take_sections() {
                            push_to_table(&mut target, &section, &atomic, sections).await;
                        }
                    } else {
                        break;
                    }
                }
                Ok(target)
            });
        }

        let joins = joins.join().await?;
        graph = AdjacencyArrayBuilder::new(&filtered_glyphs);
        for section in joins {
            graph.join(section);
        }
    }

    let graph = graph.build(1.5, |ch| find_unicode_block(ch).map(|x| x.name()));

    let name = format!("{ADJACENCY_TAG}/{ADJACENCY_VERSION}");
    let mut package = DataPackageEncoder::new(&name);
    package.insert_section(ADJACENCY_TAG, graph.serialize(&name)?);
    package.build().save(ADJACENCY_PATH)?;

    Ok(())
}
