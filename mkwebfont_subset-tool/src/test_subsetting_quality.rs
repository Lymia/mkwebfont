use crate::generate_validation_data::{VALIDATION_DATA_PATH, VALIDATION_DATA_TAG};
use anyhow::Result;
use mkwebfont::{LoadedFont, SubsetPlan, WebfontInfo};
use mkwebfont_common::{
    join_set::JoinSet,
    model::{bitset_list::BitsetList, data_package::DataPackage},
};
use roaring::RoaringBitmap;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::Mutex;
use tracing::info;

async fn measure_font(bitset: BitsetList, lock: Arc<Mutex<()>>, style: WebfontInfo) -> Result<()> {
    let mut all_chars = RoaringBitmap::new();
    for item in style.subsets() {
        all_chars.extend(item.subset().iter());
    }

    let chars_in_font = style.all_chars();
    let style = Arc::new(style);
    let mut joins = JoinSet::new();
    for section in bitset.sections() {
        let chars_in_font = chars_in_font.clone();
        let section = section.clone();
        let style = style.clone();
        joins.spawn(async move {
            let mut total_size_valid = 0u64;
            let mut total_size_all = 0u64;
            let mut total_missing = 0u64;
            let mut valid_count = 0usize;
            let mut total_subsets = 0usize;
            for bitmap in section.iter() {
                let bitmap = section.decode(bitmap);

                let mut size = 0u64;
                for subset in style.subsets() {
                    if bitmap.intersection_len(subset.subset()) > 0 {
                        size += subset.woff2_data().len() as u64;
                        total_subsets += 1;
                    }
                }

                let missing = bitmap.len() - bitmap.intersection_len(&chars_in_font);
                if missing == 0 {
                    total_size_valid += size;
                    valid_count += 1;
                }
                total_size_all += size;
                total_missing += missing;
            }

            let name = section.source();
            let valid = (valid_count as f64 / section.len() as f64) * 100.0;
            let files = (total_subsets as f64 / section.len() as f64);
            let avg_kib = (total_size_valid as f64 / valid_count as f64) / 1024.0;
            let avg_kib_all = (total_size_all as f64 / section.len() as f64) / 1024.0;
            let missing = total_missing as f64 / section.len() as f64;

            Ok((name.to_string(), valid, files, avg_kib, avg_kib_all, missing))
        });
    }

    let data = joins.join().await?;
    let _lock = lock.lock();
    println!();
    println!("===================================================================================");
    println!("Report for font: {} {}", style.font_family(), style.font_style());
    println!("===================================================================================");
    println!("Script          | %Valid | Avg.Miss | Avg.Files | Avg.Size (Valid) | Avg.Size (All)");
    println!("----------------+--------+----------+-----------+------------------+---------------");
    for (name, valid, files, avg_kib, avg_kib_all, missing) in data {
        let avg_kib = if avg_kib.is_nan() {
            "--".to_string()
        } else {
            format!("{avg_kib:.2}")
        };
        let avg_kib_all = if avg_kib_all.is_nan() {
            "--".to_string()
        } else {
            format!("{avg_kib_all:.2}")
        };
        print!("{name:-15} | {valid:5.1}% | {missing:8.2} | {files:9.3} | ");
        println!("{avg_kib:>12} KiB | {avg_kib_all:>10} KiB");
    }
    println!("===================================================================================");

    Ok(())
}

pub async fn test_subsetting_quality(paths: &[PathBuf]) -> Result<()> {
    let mut pkg = DataPackage::load(VALIDATION_DATA_PATH)?;
    let bitset = BitsetList::deserialize(pkg.take_section(VALIDATION_DATA_TAG)?)?;

    let mut loaded_fonts = Vec::new();
    for path in paths {
        loaded_fonts.extend(LoadedFont::load(&std::fs::read(path)?)?);
    }

    let plan = SubsetPlan::new();
    let styles = mkwebfont::process_webfont(&plan, &loaded_fonts).await?;

    let mutex = Arc::new(Mutex::new(()));
    let mut threads = Vec::new();
    for style in styles {
        threads.push(tokio::spawn(measure_font(bitset.clone(), mutex.clone(), style)));
    }
    for thread in threads {
        thread.await??;
    }
    println!();
    info!("Done!");

    Ok(())
}
