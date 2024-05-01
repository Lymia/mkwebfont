use crate::common_crawl_download::COMMON_CRAWL_TAG;
use anyhow::Result;
use mkwebfont_common::{
    join_set::JoinSet,
    model::{
        bitset_list,
        bitset_list::{BitsetList, BitsetSectionBuilder},
        data_package::{DataPackage, DataPackageEncoder},
    },
    wyhash::{wyhash, WyHashBuilder, WyRand},
};
use roaring::RoaringBitmap;
use std::{collections::HashSet, sync::Arc};
use unicode_blocks::find_unicode_block;

pub const VALIDATION_DATA_PATH: &str = "run/raw_validation_data";
pub const VALIDATION_DATA_TAG: &str = "validation_data";
const VALIDATION_DATA_VERSION: &str = "v0.1.0";
const VALIDATION_DATA_COUNT: usize = 25000;

#[derive(Copy, Clone)]
struct Script {
    name: &'static str,
    check_blocks: &'static [&'static str],
    exclude_blocks: &'static [&'static str],
}

/// This list isn't intending to be complete, but a rough sample of the potentially problematic
/// or extremely common language groups. The CJK languages are the main focus here, since they are
/// the ones that Google decided to take special measures for (with very good reason).
const SCRIPTS: &[Script] = &[
    Script {
        name: "Latin",
        check_blocks: &["Basic Latin", "Latin-1 Supplement"],
        exclude_blocks: &[],
    },
    Script {
        name: "Latin Extended",
        check_blocks: &[
            "Latin Extended-A",
            "Latin Extended-B",
            "Latin Extended-C",
            "Latin Extended-D",
            "Latin Extended-E",
            "Latin Extended-F",
            "Latin Extended-G",
        ],
        exclude_blocks: &[],
    },
    Script { name: "Cyrillic", check_blocks: &["Cyrillic"], exclude_blocks: &[] },
    Script {
        name: "Greek",
        check_blocks: &["Greek and Coptic", "Greek Extended"],
        exclude_blocks: &[],
    },
    Script { name: "Arabic", check_blocks: &["Arabic"], exclude_blocks: &[] },
    Script {
        name: "Chinese",
        check_blocks: &[
            "CJK Unified Ideographs",
            "CJK Unified Ideographs Extension A",
            "CJK Unified Ideographs Extension B",
            "CJK Unified Ideographs Extension C",
            "CJK Unified Ideographs Extension D",
            "CJK Unified Ideographs Extension E",
            "CJK Unified Ideographs Extension F",
            "CJK Unified Ideographs Extension G",
            "CJK Unified Ideographs Extension H",
            "CJK Unified Ideographs Extension I",
        ],
        exclude_blocks: &[
            "Hiragana",
            "Katakana",
            "Hangul Syllables",
            "Hangul Jamo",
            "Hangul Jamo Extended-A",
            "Hangul Jamo Extended-B",
        ],
    },
    Script { name: "Japanese", check_blocks: &["Hiragana", "Katakana"], exclude_blocks: &[] },
    Script {
        name: "Korean",
        check_blocks: &["Hangul Syllables", "Hangul Jamo"],
        exclude_blocks: &[],
    },
];

/// A list of endonyms for languages. Used to filter out characters used only for a language
/// selector, since this causes a lot of issues.
const ENDONYMS: &str = include_str!("endonyms.txt");

fn block_list(chars: &RoaringBitmap) -> HashSet<&'static str, WyHashBuilder> {
    let mut map = HashSet::default();
    for ch in chars {
        let block = find_unicode_block(char::from_u32(ch).unwrap()).unwrap();
        map.insert(block.name());
    }
    map
}
fn filter_endonyms(
    endonym_chars: &RoaringBitmap,
    bitmap: RoaringBitmap,
    chars: &[u32],
) -> RoaringBitmap {
    let mut ch_bitmap = RoaringBitmap::new();
    for ch in &bitmap {
        ch_bitmap.insert(chars[ch as usize]);
    }

    let excluded_chars = ch_bitmap.clone() - endonym_chars;
    let blocks_orig = block_list(&ch_bitmap);
    let blocks_removed = block_list(&excluded_chars);

    let mut removed_blocks = blocks_orig;
    for block in blocks_removed {
        removed_blocks.remove(block);
    }

    if !removed_blocks.is_empty() {
        let mut filtered = RoaringBitmap::new();
        for ch in bitmap {
            let block = find_unicode_block(char::from_u32(chars[ch as usize]).unwrap()).unwrap();
            if !removed_blocks.contains(block.name()) {
                filtered.insert(ch);
            }
        }
        filtered
    } else {
        bitmap
    }
}

fn build_block_list(bitsets: &BitsetList, list: &[&str]) -> Vec<Vec<bool>> {
    let mut cache: HashSet<_, WyHashBuilder> = HashSet::default();
    for block in list {
        cache.insert(*block);
    }

    let mut out = Vec::new();
    for section in bitsets.sections() {
        let mut data = vec![false; section.chars().len()];
        for (i, &ch) in section.chars().iter().enumerate() {
            let block = find_unicode_block(char::from_u32(ch).unwrap()).unwrap();
            data[i] = cache.contains(block.name());
        }
        out.push(data);
    }
    out
}
fn build_filtered_bitset(
    endonym_chars: &RoaringBitmap,
    script: &Script,
    bitsets: &BitsetList,
    mut count: usize,
) -> BitsetSectionBuilder {
    let mut builder = BitsetSectionBuilder::new(&script.name);

    // Prepare state for function
    let includes = build_block_list(bitsets, script.check_blocks);
    let excludes = build_block_list(bitsets, script.exclude_blocks);
    let mut fulfilled = Vec::new();
    for section in bitsets.sections() {
        fulfilled.push(vec![false; section.len()]);
    }

    // Try to find `count` different bitsets.
    let mut rand = WyRand::new(wyhash(12345, &script.name));
    while count != 0 {
        let section_idx = (rand.rand() % bitsets.sections().len() as u64) as usize;
        let section = &bitsets.sections()[section_idx];
        let bitmap_idx = (rand.rand() % section.len() as u64) as usize;
        let bitmap = section.get(bitmap_idx);
        let bitmap = filter_endonyms(endonym_chars, bitmap, section.chars());

        if fulfilled[section_idx][bitmap_idx] {
            continue;
        }
        fulfilled[section_idx][bitmap_idx] = true;

        let mut check_pass = false;
        for ch in &bitmap {
            if excludes[section_idx][ch as usize] {
                check_pass = false;
                break;
            }
            if includes[section_idx][ch as usize] {
                check_pass = true;
            }
        }

        if check_pass {
            builder.push_sample_from_bitset(section.chars(), bitmap);
            count -= 1;
        }
    }

    // Return an optimzied builder
    builder.optimize()
}

pub async fn generate_validation_data() -> Result<()> {
    // collect characters that are only used in endonyms
    let mut endonym_chars = RoaringBitmap::new();
    for ch in ENDONYMS.chars() {
        let block = find_unicode_block(ch).unwrap();
        match block.name() {
            "Basic Latin" => {}
            "Latin-1 Supplement" => {}
            _ => {
                endonym_chars.insert(ch as u32);
            }
        }
    }
    let endonym_chars = Arc::new(endonym_chars);

    // load the bitset list
    let mut data = DataPackage::load("run/common-crawl_bitsets-validation")?;
    let bitset_list = Arc::new(BitsetList::deserialize(data.take_section(COMMON_CRAWL_TAG)?)?);

    // build the validation bitset list
    let mut joins = JoinSet::new();
    for &script in SCRIPTS {
        let bitset_list = bitset_list.clone();
        let endonym_chars = endonym_chars.clone();
        joins.spawn(async move {
            Ok(build_filtered_bitset(
                &endonym_chars,
                &script,
                &bitset_list,
                VALIDATION_DATA_COUNT,
            ))
        });
    }
    let bitset_list = bitset_list::build(joins.join().await?);

    // write to disk
    let name = format!("{VALIDATION_DATA_TAG}/{VALIDATION_DATA_VERSION}");
    let mut package = DataPackageEncoder::new(&name);
    package.insert_section(VALIDATION_DATA_TAG, bitset_list.serialize(&name)?);
    package.build().save(VALIDATION_DATA_PATH)?;

    Ok(())
}
