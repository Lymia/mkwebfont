use crate::{
    data::DataStorage, fonts::FontFaceWrapper, render::FontEncoder,
    splitter::SplitterImplementation, subset_plan::LoadedSubsetPlan,
};
use anyhow::Result;
use roaring::RoaringBitmap;
use tracing::warn;

pub struct AdjacencySplitter;
impl SplitterImplementation for AdjacencySplitter {
    async fn split(
        &self,
        font: &FontFaceWrapper,
        plan: &LoadedSubsetPlan,
        encoder: &mut FontEncoder,
    ) -> Result<()> {
        let adjacency = DataStorage::instance()?.adjacency_array().await?;

        let mut raw_chars = Vec::new();
        for glyph in plan.do_subset(font.all_codepoints().clone()) {
            if let Some(glyph) = char::from_u32(glyph) {
                raw_chars.push((glyph, adjacency.get_character_frequency(glyph as u32)));
            } else {
                warn!("Not character? {glyph}");
            }
        }
        raw_chars.sort_by_key(|x| x.1);

        let mut chars = Vec::new();
        for (glyph, _) in raw_chars {
            chars.push(glyph);
        }

        let mut remaining = Vec::new();
        for &seed_ch in &chars {
            remaining.push(seed_ch as u32);
        }

        while !remaining.is_empty() {
            let seed_ch = remaining.pop().unwrap();

            let mut subset = Vec::new();
            subset.push(char::from_u32(seed_ch).unwrap());

            while subset.len() < 75 && !remaining.is_empty() {
                let best_i = (0..remaining.len())
                    .rev()
                    .take(512)
                    .max_by_key(|&i| {
                        let ch = char::from_u32(remaining[i]).unwrap();
                        adjacency.estimate_conditional_probability(&subset, ch)
                    })
                    .unwrap();
                let best_ch = char::from_u32(remaining[best_i]).unwrap();
                subset.push(best_ch);
                remaining.remove(best_i);
            }

            let mut bitmap = RoaringBitmap::new();
            for ch in subset {
                bitmap.insert(ch as u32);
            }
            encoder.add_subset(&format!("ss{}", remaining.len()), plan, bitmap);
        }

        Ok(())
    }
}
