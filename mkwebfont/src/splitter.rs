use crate::{
    fonts::LoadedFont,
    render::FontEncoder,
    subset_manifest::{WebfontSubset, WebfontSubsetGroup},
    WebfontCtx, WebfontCtxData,
};
use roaring::RoaringBitmap;
use serde::Deserialize;
use std::{collections::HashSet, sync::Arc};
use tracing::debug;
use unic_ucd_block::Block;

#[derive(Clone, Debug, Deserialize)]
pub struct TuningParameters {
    reject_subset_threshold: usize,
    accept_subset_count_threshold: usize,
    accept_subset_ratio_threshold: f64,
    accept_group_ratio_threshold: f64,
    high_priority_ratio_threshold: f64,
    high_priority_subsets: Vec<String>,
    residual_class_max_size: usize,
}

struct FontSplittingContext<'a> {
    ctx: &'a WebfontCtxData,
    font: LoadedFont,
    encoder: &'a mut FontEncoder,
    fulfilled_codepoints: RoaringBitmap,
    processed_subsets: HashSet<Arc<str>>,
    processed_groups: HashSet<Arc<str>>,
    residual_id: usize,
    preload_done: bool,
}
impl<'a> FontSplittingContext<'a> {
    fn new(ctx: &'a WebfontCtx, font: &LoadedFont, encoder: &'a mut FontEncoder) -> Self {
        FontSplittingContext {
            ctx: &ctx.0,
            font: font.clone(),
            encoder,
            fulfilled_codepoints: Default::default(),
            processed_subsets: Default::default(),
            processed_groups: Default::default(),
            residual_id: 0,
            preload_done: false,
        }
    }

    fn do_subset(&mut self, subset: &WebfontSubset) {
        if !self.processed_subsets.contains(&subset.name) {
            self.processed_subsets.insert(subset.name.clone());

            let mut name = subset.name.to_string();
            let mut new_codepoints =
                self.font.codepoints_in_set(&subset.map) - &self.fulfilled_codepoints;

            if new_codepoints.len() as usize >= self.ctx.tuning.reject_subset_threshold {
                if !self.preload_done {
                    let mut preload_list = self.ctx.preload_codepoints.clone();
                    if let Some(list) = self.ctx.preload_codepoints_in.get(self.font.font_family())
                    {
                        preload_list |= list;
                    }

                    let new = new_codepoints.clone() | preload_list;
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
                self.encoder.add_subset(&name, new_codepoints);
            } else {
                debug!("Rejecting subset: {name} (unique codepoints: {})", new_codepoints.len())
            }
        }
    }
    fn do_subset_group(&mut self, subset_group: &WebfontSubsetGroup) {
        debug!("Splitting subset group from font: {}", subset_group.name);
        if !self.processed_groups.contains(&subset_group.name) {
            self.processed_groups.insert(subset_group.name.clone());
            for subset in &subset_group.subsets {
                self.do_subset(subset);
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

    fn select_subset_group(&mut self) -> Option<Arc<WebfontSubsetGroup>> {
        let mut selected = None;
        for v in &self.ctx.data.groups {
            if !self.processed_groups.contains(&v.name) {
                let ratio = self.subset_group_ratio(v);
                if ratio != 0.0 {
                    if let Some((_, last_score)) = selected {
                        if ratio > last_score {
                            selected = Some((v, ratio));
                        }
                    } else {
                        selected = Some((v, ratio));
                    }
                }
            }
        }
        if let Some((subset, ratio)) = selected {
            if ratio >= self.ctx.tuning.accept_group_ratio_threshold {
                Some(subset.clone())
            } else {
                debug!("Omitted subset group {} - ratio: {:.4}", subset.name, ratio);
                None
            }
        } else {
            None
        }
    }
    fn select_next_subset(&mut self) -> Option<Arc<WebfontSubset>> {
        let mut subsets = Vec::new();
        for v in &self.ctx.data.subsets {
            if !self.processed_subsets.contains(&v.name) {
                let count = self.unique_available_count(v);
                let ratio = self.unique_available_ratio(v);
                subsets.push((v, count, ratio));
            }
        }

        let mut top = Vec::new();
        subsets.sort_by_key(|x| -(x.1 as isize));
        top.extend(subsets.first().cloned());
        subsets.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());
        top.extend(subsets.first().cloned());
        top.dedup_by(|x, y| x.0.name == y.0.name);

        for (subset, count, ratio) in &top {
            if *ratio >= self.ctx.tuning.accept_subset_ratio_threshold
                || *count >= self.ctx.tuning.accept_subset_count_threshold
            {
                debug!("Selecting subset {} - count: {count}, ratio: {ratio:.4}", subset.name);
                return Some((*subset).clone());
            }
        }
        for (subset, count, ratio) in top {
            debug!("Omitted subset {} - count: {count}, ratio: {ratio:.4}", subset.name);
        }
        None
    }

    fn check_high_priority(&mut self) {
        for name in self.ctx.tuning.high_priority_subsets.clone() {
            let name = name.as_str();
            debug!("Checking high priority subset: {name}");
            let subset = self.ctx.data.by_name.get(name).unwrap().clone();
            if self.unique_available_ratio(&subset) > self.ctx.tuning.high_priority_ratio_threshold
            {
                self.do_subset(&subset);
            }
        }
    }

    fn split_to_blocks(codepoints: RoaringBitmap) -> Vec<RoaringBitmap> {
        let mut last_glyph_block = None;
        let mut accum = RoaringBitmap::new();
        let mut list = Vec::new();
        for glyph in codepoints {
            let block = Block::of(char::from_u32(glyph).unwrap()).map(|x| x.name);
            if last_glyph_block != block && !accum.is_empty() {
                list.push(std::mem::replace(&mut accum, RoaringBitmap::new()));
            }
            last_glyph_block = block;
            accum.insert(glyph);
        }
        if !accum.is_empty() {
            list.push(accum);
        }
        list
    }
    fn generate_residual_block(&mut self, residual: &mut Vec<RoaringBitmap>) {
        let mut set = RoaringBitmap::new();
        let mut i = 0;
        while i < residual.len() {
            if residual[i].len() as usize > self.ctx.tuning.residual_class_max_size
                && set.is_empty()
            {
                set = residual[i]
                    .iter()
                    .take(self.ctx.tuning.residual_class_max_size)
                    .collect();
                residual[i] = residual[i].clone() - set.clone();
                break;
            } else if (set.len() + residual[i].len()) as usize
                <= self.ctx.tuning.residual_class_max_size
            {
                set.extend(residual.remove(i));
            } else {
                i += 1;
            }
        }

        assert!(!set.is_empty());
        let name = format!("misc{}", self.residual_id);
        self.residual_id += 1;
        self.encoder.add_subset(&name, set);
    }
    fn split_residual(&mut self) {
        let codepoints = self.font.all_codepoints() - &self.fulfilled_codepoints;
        if !codepoints.is_empty() {
            debug!(
                "Splitting residual codepoints into subsets (remaining codepoints: {})",
                codepoints.len()
            );
            let mut split = Self::split_to_blocks(codepoints);
            while !split.is_empty() {
                self.generate_residual_block(&mut split);
            }
        }
    }
}

pub fn split_webfonts(ctx: &WebfontCtx, font: &LoadedFont, encoder: &mut FontEncoder) {
    debug!("Beginning splitting fonts...");
    let mut ctx = FontSplittingContext::new(ctx, font, encoder);
    ctx.check_high_priority();
    while let Some(subset_group) = ctx.select_subset_group() {
        ctx.do_subset_group(&subset_group);
    }
    while let Some(subset) = ctx.select_next_subset() {
        ctx.do_subset(&subset);
    }
    ctx.split_residual();
    debug!("Font splitting complete. Awaiting encoding...");
}
