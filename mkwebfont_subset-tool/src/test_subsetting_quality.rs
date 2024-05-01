use crate::common_crawl_download::COMMON_CRAWL_TAG;
use anyhow::Result;
use mkwebfont::{LoadedFont, WebfontCtxBuilder, WebfontInfo};
use mkwebfont_common::model::{bitset_list::BitsetList, data_package::DataPackage};
use roaring::RoaringBitmap;
use std::{ops::RangeInclusive, path::PathBuf, sync::Arc};
use tokio::sync::Mutex;
use tracing::{debug, info};
use unicode_properties::{GeneralCategoryGroup, UnicodeEmoji, UnicodeGeneralCategory};

struct Script {
    name: &'static str,
    ranges: &'static [RangeInclusive<char>],
}

/// This list isn't intending to be complete, but a rough sample of the potentially problematic
/// or extremely common language groups. The CJK languages are the main focus here, since they are
/// the ones that Google decided to take special measures for (with very good reason).
const SCRIPTS: &[Script] = &[
    Script { name: "Latin", ranges: &['a'..='z', 'A'..='Z'] },
    Script {
        name: "Latin Extended", ranges: &['À'..='Ö', 'Ø'..='ö', 'ø'..='ɏ', 'Ḁ'..='ỿ']
    },
    Script {
        name: "Latin Phonetics", // IPA and dictionary phonetics
        ranges: &['ɐ'..='ʯ', 'ᴀ'..='ᶿ'],
    },
    Script { name: "Cyrillic", ranges: &['Ѐ'..='ӿ'] },
    Script { name: "Greek", ranges: &['Ͱ'..='Ͽ'] },
    Script {
        name: "Arabic",
        ranges: &['\u{600}'..='\u{6FF}'], // escaped to avoid RTL problems
    },
    Script {
        name: "Hebrew",
        ranges: &['\u{590}'..='\u{5FF}'], // escaped to avoid RTL problems
    },
    Script {
        name: "CJK", ranges: &['一'..='鿿', '㐀'..='䶿', '𠀀'..='𪛖', '𪜀'..='𫜴']
    },
    Script { name: "Japanese", ranges: &['ぁ'..='ゖ', 'ァ'..='ヺ'] },
    Script { name: "Korean", ranges: &['ᄀ'..='ᇿ', '가'..='힣'] },
];

async fn measure_font(lock: Arc<Mutex<()>>, style: WebfontInfo) -> Result<()> {
    let mut all_chars = RoaringBitmap::new();
    for item in style.subsets() {
        all_chars.extend(item.subset().iter());
    }

    let mut data = DataPackage::load("run/common-crawl_bitsets-validation")?;
    let bitset_list = BitsetList::deserialize(data.take_section(COMMON_CRAWL_TAG)?)?;

    // define and create the statistics table
    #[derive(Copy, Clone)]
    struct ScriptStatistics {
        name: &'static str,
        valid_count: usize,
        invalid_count: usize,
        bytes: u64,
        bytes_all: u64,
    }
    impl ScriptStatistics {
        fn apply(&mut self, is_valid: bool, total_size: u64) {
            if is_valid {
                self.valid_count += 1;
                self.bytes += total_size;
            } else {
                self.invalid_count += 1;
            }
            self.bytes_all += total_size;
        }
    }
    let mut statistics =
        [ScriptStatistics { name: "", valid_count: 0, invalid_count: 0, bytes: 0, bytes_all: 0 };
            SCRIPTS.len() + 2];
    for (i, script) in SCRIPTS.iter().enumerate() {
        statistics[i].name = script.name;
    }
    statistics[SCRIPTS.len()].name = "Emoji";
    statistics[SCRIPTS.len() + 1].name = "All Webpages";

    for subset in style.subsets() {
        debug!("{} / {:.2} KiB", subset.name(), subset.woff2_data().len() as f64 / 1024.0);
    }

    // execute the actual counting operaiton
    let mut processed = 0;
    for (bitmap, chars) in bitset_list.iter().take(300000) {
        processed += 1;
        if processed % 100000 == 0 {
            debug!("Processed {processed} bitmaps...");
        }

        // Filter the bitmap to remove whitespace and control characters.
        let mut filtered_bitmap = RoaringBitmap::new();
        let mut has_emoji = false;
        for idx in &bitmap {
            let ch = char::from_u32(chars[idx as usize]).unwrap();
            let cat = ch.general_category_group();
            if cat == GeneralCategoryGroup::Separator || cat == GeneralCategoryGroup::Other {
                continue;
            }
            if ch == '�' || ch == '\0' {
                continue;
            }
            if !has_emoji && ch.is_emoji_char() {
                has_emoji = true;
            }
            filtered_bitmap.insert(ch as u32);
        }

        // Check if the font is valid at all and calculate the font size used
        let total_size = {
            let mut total_size = 0u64;
            for subset in style.subsets() {
                if !subset.subset().is_disjoint(&bitmap) {
                    total_size += subset.woff2_data().len() as u64;
                }
            }
            total_size
        };

        // reasonable margin for pages that contain small amounts of other languages
        let is_valid = all_chars.is_superset(&filtered_bitmap);

        // Categorize the scripts this webpage falls under
        for (i, script) in SCRIPTS.iter().enumerate() {
            let matches = script
                .ranges
                .iter()
                .any(|x| bitmap.range_cardinality(*x.start() as u32..=*x.end() as u32) > 0);
            if matches {
                statistics[i].apply(is_valid, total_size);
            }
        }
        if has_emoji {
            statistics[SCRIPTS.len()].apply(is_valid, total_size)
        }
        statistics[SCRIPTS.len() + 1].apply(is_valid, total_size);
    }

    let _guard = lock.lock().await;

    println!();
    println!();
    println!("Report for font: {} {}", style.font_family(), style.font_style());
    println!();
    println!("Script          | Total Pages | %Valid | Avg.Size (Valid) | Avg.Size (All)");
    println!("----------------+-------------+--------+------------------+---------------");
    for ScriptStatistics { name, valid_count, invalid_count, bytes, bytes_all } in statistics {
        let total = valid_count + invalid_count;
        let avg_kib = (bytes as f64 / valid_count as f64) / 1024.0;
        let avg_kib_all = (bytes_all as f64 / total as f64) / 1024.0;
        let valid = (valid_count as f64 / total as f64) * 100.0;

        print!("{name:-15} | {total:11} | {valid:5.1}% | ");

        if valid_count == 0 {
            print!("             KiB | ");
        } else {
            print!("{avg_kib:12.2} KiB | ");
        }
        println!("{avg_kib_all:10.2} KiB");
    }

    Ok(())
}

pub async fn test_subsetting_quality(paths: &[PathBuf]) -> Result<()> {
    let mut loaded_fonts = Vec::new();
    for path in paths {
        loaded_fonts.extend(LoadedFont::load(&std::fs::read(path)?)?);
    }

    let ctx = WebfontCtxBuilder::new().build()?;
    let styles = mkwebfont::process_webfont(&ctx, &loaded_fonts).await?;

    let mutex = Arc::new(Mutex::new(()));
    let mut threads = Vec::new();
    for style in styles {
        threads.push(tokio::spawn(measure_font(mutex.clone(), style)));
    }
    for thread in threads {
        thread.await??;
    }
    println!();
    info!("Done!");

    Ok(())
}
