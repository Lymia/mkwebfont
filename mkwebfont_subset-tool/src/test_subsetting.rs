use crate::generate_data::VERSION;
use anyhow::Result;
use mkwebfont::LoadedFont;
use mkwebfont_common::{adjacency_bloom_filter::AdjacencyBloomFilter, data_package::DataPackage};
use roaring::RoaringBitmap;
use std::path::PathBuf;
use tracing::info;

fn subset(font: &LoadedFont) -> Result<()> {
    info!("Loading data...");
    let data = std::fs::read(format!("run/mkwebfont_data-{VERSION}"))?;
    let data = DataPackage::deserialize(&data)?;
    let bloom = AdjacencyBloomFilter::deserialize(&data, "main")?;

    info!("Font: {} {}", font.font_family(), font.font_style());

    let mut characters = Vec::new();
    for glyph in font.codepoints() {
        characters.push(char::from_u32(glyph).unwrap());
    }

    let mut frequency_order = Vec::new();
    for &ch in bloom.glyph_list() {
        if characters.contains(&ch) {
            frequency_order.push((ch, bloom.get_character_frequency(ch as u32)));
        }
    }
    frequency_order.sort_by_key(|x| x.1);
    frequency_order.reverse();

    let mut fulfilled = RoaringBitmap::new();
    let mut remaining = RoaringBitmap::new();
    for &(seed_ch, _) in &frequency_order {
        remaining.insert(seed_ch as u32);
    }
    for &(seed_ch, _) in &frequency_order {
        if !fulfilled.contains(seed_ch as u32) {
            let mut subset = Vec::new();
            subset.push(seed_ch);
            fulfilled.insert(seed_ch as u32);
            remaining.remove(seed_ch as u32);

            while subset.len() < 50 && !remaining.is_empty() {
                let mut best_modularity = 0.0;
                let mut best_ch = None;
                for ch in &remaining {
                    let ch = char::from_u32(ch).unwrap();

                    let modularity = bloom.delta_modularity(ch, &subset);
                    if modularity > best_modularity {
                        best_modularity = modularity;
                        best_ch = Some(ch);
                    }
                }

                if best_ch.is_none() {
                    break;
                }

                let best_ch = best_ch.unwrap();
                subset.push(best_ch);
                fulfilled.insert(best_ch as u32);
                remaining.remove(best_ch as u32);
            }

            let mut str = String::new();
            for ch in subset {
                str.push(ch);
            }
            println!("{str:?}");
        }
    }

    Ok(())
}

pub fn test_subsetting(paths: &[PathBuf]) -> Result<()> {
    let mut loaded_fonts = Vec::new();
    for path in paths {
        loaded_fonts.extend(LoadedFont::load(&std::fs::read(path)?)?);
    }
    for font in loaded_fonts {
        subset(&font)?;
    }
    Ok(())
}
