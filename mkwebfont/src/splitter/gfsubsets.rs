use crate::{
    data::DataStorage,
    plan::{AssignedSubsets, LoadedSplitterPlan},
    splitter::SplitterImplementation,
};
use anyhow::Result;
use mkwebfont_common::model::subset_data::{WebfontData, WebfontSubset, WebfontSubsetGroup};
use mkwebfont_fontops::{font_info::FontFaceWrapper, subsetter::FontEncoder};
use ordered_float::OrderedFloat;
use roaring::RoaringBitmap;
use std::{collections::HashSet, sync::Arc};
use tracing::debug;
use unicode_blocks::find_unicode_block;

#[derive(Copy, Clone, Debug)]
pub struct TuningParameters {
    reject_subset_threshold: usize,
    accept_subset_count_threshold: usize,
    accept_subset_ratio_threshold: f64,
    accept_group_ratio_threshold: f64,
    high_priority_ratio_threshold: f64,
    high_priority_subsets: &'static [&'static str],
    residual_class_max_size: usize,
}

const DEFAULT_TUNING: TuningParameters = TuningParameters {
    reject_subset_threshold: 20,
    accept_subset_count_threshold: 20,
    accept_subset_ratio_threshold: 0.1,
    accept_group_ratio_threshold: 0.25,
    high_priority_ratio_threshold: 0.25,
    high_priority_subsets: &["latin", "latin-ext"],
    residual_class_max_size: 200,
};

struct SplitterState {
    font: FontFaceWrapper,
    tuning: TuningParameters,
    data: Arc<WebfontData>,

    fulfilled_codepoints: RoaringBitmap,
    preload_codepoints: RoaringBitmap,
    processed_subsets: HashSet<Arc<str>>,
    processed_groups: HashSet<Arc<str>>,
    misc_idx: usize,
    preload_done: bool,
}
impl SplitterState {
    async fn init(font: &FontFaceWrapper, assigned: &AssignedSubsets) -> Result<SplitterState> {
        let fulfilled = font.all_codepoints() - assigned.get_used_chars(font);
        Ok(SplitterState {
            font: font.clone(),
            tuning: DEFAULT_TUNING,
            data: DataStorage::instance()?.gfsubsets().await?,
            fulfilled_codepoints: fulfilled,
            preload_codepoints: assigned.get_preload_chars(font),
            processed_subsets: Default::default(),
            processed_groups: Default::default(),
            misc_idx: 0,
            preload_done: false,
        })
    }

    /// Applies a single subset
    fn do_subset(&mut self, subset: &WebfontSubset, encoder: &mut FontEncoder, never_reject: bool) {
        if !self.processed_subsets.contains(&subset.name) {
            self.processed_subsets.insert(subset.name.clone());

            let mut name = subset.name.to_string();
            let mut new_codepoints =
                self.font.codepoints_in_set(&subset.map) - &self.fulfilled_codepoints;

            if never_reject || new_codepoints.len() as usize >= self.tuning.reject_subset_threshold
            {
                if !self.preload_done {
                    let new = new_codepoints.clone() | &self.preload_codepoints;
                    if new != new_codepoints {
                        name = format!("{name}+pl");
                        debug!(
                            "Preloading {} codepoints in {name}",
                            new.len() - new_codepoints.len()
                        );
                        new_codepoints = new;
                    }
                    self.preload_done = true;
                }

                self.fulfilled_codepoints.extend(new_codepoints.clone());
                encoder.add_subset(&name, new_codepoints);
            } else {
                debug!("Rejecting subset: {name} (unique codepoints: {})", new_codepoints.len())
            }
        }
    }

    /// Applies a subset group
    fn do_subset_group(&mut self, subset_group: &WebfontSubsetGroup, encoder: &mut FontEncoder) {
        debug!("Splitting subset group from font: {}", subset_group.name);
        if !self.processed_groups.contains(&subset_group.name) {
            self.processed_groups.insert(subset_group.name.clone());
            for subset in &subset_group.subsets {
                self.do_subset(subset, encoder, false);
            }
        }
    }

    fn unique_available_ratio(&self, subset: &WebfontSubset) -> f64 {
        let present_codepoints =
            (self.font.codepoints_in_set(&subset.map) - &self.fulfilled_codepoints).len();
        let subset_codepoints = (subset.map.clone() - &self.fulfilled_codepoints).len();
        if subset_codepoints == 0 {
            0.0
        } else {
            (present_codepoints as f64) / (subset_codepoints as f64)
        }
    }
    fn unique_available_count(&self, subset: &WebfontSubset) -> usize {
        let present_codepoints =
            (self.font.codepoints_in_set(&subset.map) - &self.fulfilled_codepoints).len();
        present_codepoints as usize
    }
    fn subset_group_ratio(&self, group: &WebfontSubsetGroup) -> f64 {
        let mut accum = 0.0;
        for subset in &group.subsets {
            accum += self.unique_available_ratio(subset);
        }
        accum / group.subsets.len() as f64
    }

    /// Selects the best subset group to apply.
    fn select_subset_group(&mut self) -> Option<Arc<WebfontSubsetGroup>> {
        if self.data.groups.len() == self.processed_groups.len() {
            return None;
        }

        let (group, ratio) = self
            .data
            .groups
            .iter()
            .filter(|x| !self.processed_groups.contains(&x.name))
            .map(|x| (x, self.subset_group_ratio(&*x)))
            .max_by_key(|x| OrderedFloat(x.1))
            .unwrap();

        if ratio >= self.tuning.accept_group_ratio_threshold {
            Some(group.clone())
        } else {
            debug!("Omitted subset group {} - ratio: {:.4}", group.name, ratio);
            None
        }
    }

    /// Selects the best subset to apply.
    fn select_next_subset(&mut self) -> Option<Arc<WebfontSubset>> {
        if self.data.subsets.len() == self.processed_subsets.len() {
            return None;
        }

        let (ratio_subset, best_ratio) = self
            .data
            .subsets
            .iter()
            .filter(|x| !self.processed_subsets.contains(&x.name))
            .map(|x| (x, self.unique_available_ratio(&*x)))
            .max_by_key(|x| OrderedFloat(x.1))
            .unwrap();
        let (count_subset, best_count) = self
            .data
            .subsets
            .iter()
            .filter(|x| !self.processed_subsets.contains(&x.name))
            .map(|x| (x, self.unique_available_count(&*x)))
            .max_by_key(|x| x.1)
            .unwrap();

        if best_ratio >= self.tuning.accept_subset_ratio_threshold {
            Some(ratio_subset.clone())
        } else if best_count >= self.tuning.accept_subset_count_threshold {
            Some(count_subset.clone())
        } else {
            None
        }
    }

    /// Applies high priority subsets immediately.
    fn check_high_priority(&mut self, encoder: &mut FontEncoder) {
        for &name in self.tuning.high_priority_subsets {
            if self.data.by_name.contains_key(name) {
                debug!("Checking high priority subset: {name}");
                let subset = self.data.by_name.get(name).unwrap().clone();
                if self.unique_available_ratio(&subset) > self.tuning.high_priority_ratio_threshold
                {
                    self.do_subset(&subset, encoder, false);
                }
            }
        }
    }

    /// Splits the residual class into transient subsets that will be merged again to form the
    /// final `misc` subsets.
    fn generate_transient_subsets(&mut self) -> Vec<RoaringBitmap> {
        let mut remaining = self.font.all_codepoints() - &self.fulfilled_codepoints;
        let mut subsets = Vec::new();

        // apply every subset in gfsubsets that's large enough, and create a transient subset
        for group in self
            .data
            .subsets
            .iter()
            .chain(self.data.groups.iter().flat_map(|x| &x.subsets))
        {
            let union = remaining.clone() & group.map.clone();
            if union.len() as usize > self.tuning.reject_subset_threshold / 2 {
                subsets.push(union.clone());
                remaining = remaining - union;
            }
        }

        // create remaining transient subsets by block
        while let Some(seed) = remaining.min() {
            let mut subset = RoaringBitmap::new();
            remaining.remove(seed);
            subset.insert(seed);

            let block = find_unicode_block(char::from_u32(seed).unwrap());

            while let Some(new) = remaining.min() {
                if block == find_unicode_block(char::from_u32(new).unwrap()) {
                    remaining.remove(new);
                    subset.insert(new);

                    if (subset.len() as usize) >= self.tuning.residual_class_max_size {
                        break;
                    }
                } else {
                    break;
                }
            }

            subsets.push(subset);
        }

        subsets
    }
    fn push_residual_block(&mut self, encoder: &mut FontEncoder, bitset: RoaringBitmap) {
        self.misc_idx += 1;
        self.do_subset(
            &WebfontSubset { name: format!("misc{}", self.misc_idx).into(), map: bitset },
            encoder,
            true,
        );
    }
    fn generate_residual_blocks(&mut self, encoder: &mut FontEncoder) {
        let mut transient = self.generate_transient_subsets();

        // Apply any subsets that are larger than the size limit immediately.
        {
            let mut i = 0;
            while i < transient.len() {
                if transient[i].len() as usize >= self.tuning.residual_class_max_size {
                    self.push_residual_block(encoder, transient.remove(i));
                } else {
                    i += 1;
                }
            }
        }

        // Repeately take subsets from the list until all are consumed
        while !transient.is_empty() {
            let mut new_subset = RoaringBitmap::new();

            let mut i = 0;
            while i < transient.len() {
                if (transient[i].len() + new_subset.len()) as usize
                    <= self.tuning.residual_class_max_size
                {
                    new_subset.extend(transient.remove(i));
                } else {
                    i += 1;
                }
            }

            self.push_residual_block(encoder, new_subset);
        }
    }
    fn split_resiudal(&mut self, encoder: &mut FontEncoder) {
        let codepoints = self.font.all_codepoints() - &self.fulfilled_codepoints;
        if !codepoints.is_empty() {
            debug!(
                "Splitting residual codepoints into subsets (remaining codepoints: {})",
                codepoints.len(),
            );
            self.generate_residual_blocks(encoder);
        }
    }
}

pub struct GfSubsetSplitter;
impl SplitterImplementation for GfSubsetSplitter {
    async fn split(
        &self,
        font: &FontFaceWrapper,
        _plan: &LoadedSplitterPlan,
        assigned: &AssignedSubsets,
        encoder: &mut FontEncoder,
    ) -> Result<()> {
        let mut ctx = SplitterState::init(font, assigned).await?;
        ctx.check_high_priority(encoder);
        while let Some(subset_group) = ctx.select_subset_group() {
            ctx.do_subset_group(&subset_group, encoder);
        }
        while let Some(subset) = ctx.select_next_subset() {
            ctx.do_subset(&subset, encoder, false);
        }
        ctx.split_resiudal(encoder);
        Ok(())
    }
}
