use crate::{
    font_ops::LoadedFont,
    ranges::{WebfontDataCtx, WebfontSubset, WebfontSubsetGroup},
};
use anyhow::*;
use roaring::RoaringBitmap;
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::Write,
};
use tracing::{debug, info};

#[derive(Clone, Debug, Deserialize)]
struct TuningParameters {
    reject_subset_threshold: usize,
    accept_subset_count_threshold: usize,
    accept_subset_ratio_threshold: f64,
    accept_group_ratio_threshold: f64,
    high_priority_ratio_threshold: f64,
    high_priority_subsets: Vec<String>,
    residual_class_max_size: usize,
    residual_class_preferred_size: usize,
}

struct FontSplittingContext<'a> {
    tuning: TuningParameters,
    font: LoadedFont<'a>,
    data: &'static WebfontDataCtx,
    fulfilled_glyphs: RoaringBitmap,
    woff2_subsets: HashMap<String, Vec<u8>>,
    processed_groups: HashSet<&'static str>,
}
impl<'a> FontSplittingContext<'a> {
    fn new(
        tuning: &TuningParameters,
        data: &'static WebfontDataCtx,
        font: &'a [u8],
    ) -> Result<Self> {
        debug!("Font splitting tuning parameters: {tuning:#?}");
        Ok(FontSplittingContext {
            tuning: tuning.clone(),
            font: LoadedFont::new(font)?,
            data,
            fulfilled_glyphs: Default::default(),
            woff2_subsets: Default::default(),
            processed_groups: Default::default(),
        })
    }

    fn do_subset(&mut self, subset: &'static WebfontSubset) -> Result<()> {
        if !self.woff2_subsets.contains_key(subset.name) {
            let new_glyphs = self.font.glyphs_in_font(&subset.map) - &self.fulfilled_glyphs;

            if new_glyphs.len() as usize >= self.tuning.reject_subset_threshold {
                let subset_woff2 = self.font.subset(&subset.map)?;
                info!(
                    "Splitting subset from font: {} (unique glyphs: {})",
                    subset.name,
                    new_glyphs.len()
                );
                self.fulfilled_glyphs.extend(new_glyphs);
                self.woff2_subsets
                    .insert(subset.name.to_string(), subset_woff2);
            } else {
                debug!("Rejecting subset: {} (unique glyphs: {})", subset.name, new_glyphs.len())
            }
        }
        Ok(())
    }
    fn do_subset_group(&mut self, subset_group: &'static WebfontSubsetGroup) -> Result<()> {
        info!("Splitting subset group from font: {}", subset_group.name);
        if !self.processed_groups.contains(subset_group.name) {
            self.processed_groups.insert(subset_group.name);
            for subset in &subset_group.subsets {
                self.do_subset(subset)?;
            }
        }
        Ok(())
    }

    fn unique_available_ratio(&self, subset: &'static WebfontSubset) -> f64 {
        let present_glyphs = (self.font.glyphs_in_font(&subset.map) - &self.fulfilled_glyphs).len();
        let subset_glyphs = (subset.map.clone() - &self.fulfilled_glyphs).len();
        (present_glyphs as f64) / (subset_glyphs as f64)
    }
    fn unique_available_count(&self, subset: &'static WebfontSubset) -> usize {
        let present_glyphs = (self.font.glyphs_in_font(&subset.map) - &self.fulfilled_glyphs).len();
        present_glyphs as usize
    }
    fn subset_group_ratio(&self, group: &'static WebfontSubsetGroup) -> f64 {
        let mut accum = 0.0;
        for subset in &group.subsets {
            accum += self.unique_available_ratio(subset);
        }
        accum / group.subsets.len() as f64
    }

    fn select_subset_group(&mut self) -> Result<Option<&'static WebfontSubsetGroup>> {
        let mut selected = None;
        for v in &self.data.groups {
            if !self.processed_groups.contains(v.name) {
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
            if ratio >= self.tuning.accept_group_ratio_threshold {
                Ok(Some(subset))
            } else {
                debug!("Omitted subset group {} - unique glyphs ratio {}", subset.name, ratio);
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }
    fn select_next_subset(&mut self) -> Result<Option<&'static WebfontSubset>> {
        let mut selected = None;
        for v in &self.data.subsets {
            if !self.woff2_subsets.contains_key(v.name) {
                let count = self.unique_available_count(v);
                if count != 0 {
                    let ratio = self.unique_available_ratio(v);
                    if let Some((_, last_score, _)) = selected {
                        if count > last_score {
                            selected = Some((v, count, ratio));
                        }
                    } else {
                        selected = Some((v, count, ratio));
                    }
                }
            }
        }
        if let Some((subset, count, ratio)) = selected {
            if ratio >= self.tuning.accept_subset_ratio_threshold
                || count >= self.tuning.accept_subset_count_threshold
            {
                Ok(Some(subset))
            } else {
                debug!("Omitted subset {} - unique glyphs ratio {}", subset.name, ratio);
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    fn check_high_priority(&mut self) -> Result<()> {
        for name in self.tuning.high_priority_subsets.clone() {
            let name = name.as_str();
            debug!("Checking high priority subset: {name}");
            let subset = self.data.by_name.get(name).unwrap();
            if self.unique_available_ratio(subset) > self.tuning.high_priority_ratio_threshold {
                self.do_subset(subset)?;
            }
        }
        Ok(())
    }

    fn split_residual(&mut self) -> Result<()> {
        let glyphs = self.font.all_glyphs() - &self.fulfilled_glyphs;
        if !glyphs.is_empty() {
            info!("Splitting subset from font: residual (unique glyphs: {})", glyphs.len());
            let subset_woff2 = self.font.subset(&glyphs)?;
            self.woff2_subsets
                .insert("residual".to_string(), subset_woff2);
        }
        Ok(())
    }
}

/// The internal function that actually splits the webfont.
pub fn split_webfont(
    tuning: Option<&str>,
    data: &'static WebfontDataCtx,
    font_data: &[u8],
) -> Result<()> {
    let tuning = match tuning {
        Some(x) => toml::from_str(x)?,
        None => toml::from_str(DEFAULT_TUNING_PARAMETERS)?,
    };
    let mut ctx = FontSplittingContext::new(&tuning, data, font_data)?;
    ctx.check_high_priority()?;
    while let Some(subset_group) = ctx.select_subset_group()? {
        ctx.do_subset_group(subset_group)?;
    }
    while let Some(subset) = ctx.select_next_subset()? {
        ctx.do_subset(subset)?;
    }
    ctx.split_residual()?;

    for (k, v) in &ctx.woff2_subsets {
        let mut file = File::create(format!("run/{k}.woff2"))?;
        file.write_all(v.as_slice())?;
    }

    Ok(())
}

/// The default tuning parameters for the splitter.
pub const DEFAULT_TUNING_PARAMETERS: &str = include_str!("splitter_default_tuning.toml");
