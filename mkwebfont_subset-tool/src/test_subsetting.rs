use anyhow::Result;
use mkwebfont::LoadedFont;
use mkwebfont_common::model::{adjacency_array::AdjacencyArray, data_package::DataPackage};
use roaring::RoaringBitmap;
use std::path::PathBuf;
use tracing::info;

fn subset(font: &LoadedFont) -> Result<()> {
    info!("Loading data...");
    let data = std::fs::read(crate::generate_adjacency_table::RAW_ADJACENCY_PATH)?;
    let mut data = DataPackage::deserialize(&data)?;
    let adjacency = AdjacencyArray::deserialize("raw_adjacency", &mut data)?;

    info!("Font: {} {}", font.font_family(), font.font_style());

    let mut raw_chars = Vec::new();
    for glyph in font.codepoints() {
        raw_chars.push((char::from_u32(glyph).unwrap(), adjacency.get_character_frequency(glyph)));
    }
    raw_chars.sort_by_key(|x| x.1);
    raw_chars.reverse();

    let mut chars = Vec::new();
    for (glyph, _) in raw_chars {
        chars.push(glyph);
    }

    let mut fulfilled = RoaringBitmap::new();
    let mut remaining = RoaringBitmap::new();
    for &seed_ch in &chars {
        remaining.insert(seed_ch as u32);
    }
    for &seed_ch in &chars {
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

                    let modularity = adjacency.delta_modularity(ch, &subset);
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

            let modularity = adjacency.modularity(&subset);
            let mut str = String::new();
            for ch in subset {
                str.push(ch);
            }
            println!("{str:?} {modularity}");
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
