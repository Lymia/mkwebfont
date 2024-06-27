use crate::{
    data::DataStorage,
    plan::{AssignedSubsets, LoadedSplitterPlan},
    splitter::SplitterImplementation,
};
use anyhow::Result;
use mkwebfont_fontops::{font_info::FontFaceWrapper, subsetter::FontEncoder};
use roaring::RoaringBitmap;

const TUNING_SUBSET_SIZE: usize = 100;
const TUNING_SUBSET_OVERHEAD: f64 = 2.0;

// TODO: This is still kinda trash. Fix it?

pub struct AdjacencySplitter;
impl SplitterImplementation for AdjacencySplitter {
    async fn split(
        &self,
        font: &FontFaceWrapper,
        _plan: &LoadedSplitterPlan,
        assigned: &AssignedSubsets,
        encoder: &mut FontEncoder,
    ) -> Result<()> {
        let all_chars = assigned.get_used_chars(font);

        let adjacency = DataStorage::instance()?.adjacency_array().await?;

        let mut raw_chars = Vec::new();
        for glyph in all_chars {
            raw_chars.push((glyph, adjacency.get_character_frequency(glyph as u32)));
        }
        raw_chars.sort_by_key(|x| x.1);
        let mut raw_chars: Vec<_> = raw_chars.into_iter().map(|x| x.0).collect();

        let mut active_subsets: Vec<Vec<u32>> = Vec::new();
        let mut completed_subsets: Vec<Vec<u32>> = Vec::new();

        for current_ch in raw_chars.drain(..) {
            let freq = adjacency.get_character_frequency(current_ch);
            let result = (0..active_subsets.len())
                .map(|i| (i, adjacency.estimate_wasted_space_score(&active_subsets[i], current_ch)))
                .min_by_key(|&i| i.1);
            if let Some((best_i, score)) = result {
                let overhead = (freq as f64 * TUNING_SUBSET_OVERHEAD).round() as u64;
                if score > overhead {
                    active_subsets.push(vec![current_ch]);
                } else {
                    active_subsets[best_i].push(current_ch);
                    if active_subsets[best_i].len() >= TUNING_SUBSET_SIZE {
                        completed_subsets.push(active_subsets.swap_remove(best_i));
                    }
                }
            } else {
                active_subsets.push(vec![current_ch]);
            }
        }

        completed_subsets.extend(active_subsets);
        for (i, mut subset) in completed_subsets.into_iter().enumerate() {
            subset.sort();

            let mut str = String::new();
            for ch in &subset {
                str.push(char::from_u32(*ch).unwrap());
            }
            println!("{str:?}");

            let mut bitmap = RoaringBitmap::new();
            for ch in subset {
                bitmap.insert(ch);
            }
            encoder.add_subset(&format!("ss{i}"), bitmap);
        }

        Ok(())
    }
}
