use crate::{
    font_ops::LoadedFont,
    ranges::{WebfontDataCtx, WebfontSubset},
};
use anyhow::*;
use roaring::RoaringBitmap;
use std::{collections::HashMap, fs::File, io::Write, path::PathBuf};
use tracing::{debug, info};

const HIGH_PRIORITY: &[&str] = &["latin", "latin-ext"];

struct FontSplittingContext<'a> {
    font: LoadedFont<'a>,
    data: &'static WebfontDataCtx,
    fulfilled_glyphs: RoaringBitmap,
    woff2_subsets: HashMap<&'static str, Vec<u8>>,
}
impl<'a> FontSplittingContext<'a> {
    fn new(data: &'static WebfontDataCtx, font: &'a [u8]) -> Result<Self> {
        Ok(FontSplittingContext {
            font: LoadedFont::new(font)?,
            data,
            fulfilled_glyphs: Default::default(),
            woff2_subsets: Default::default(),
        })
    }

    fn do_subset(&mut self, subset: &'static WebfontSubset) -> Result<()> {
        if !self.woff2_subsets.contains_key(subset.name) {
            let subset_woff2 = self.font.subset(&subset.map)?;
            let new_glyphs = self.font.glyphs_in_font(&subset.map) - &self.fulfilled_glyphs;

            info!(
                "Splitting subset from font: {} (unique glyphs: {})",
                subset.name,
                new_glyphs.len()
            );

            self.fulfilled_glyphs.extend(new_glyphs);
            self.woff2_subsets.insert(subset.name, subset_woff2);
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
            if ratio >= 0.1 || count >= 20 {
                Ok(Some(subset))
            } else {
                debug!("Omitted subset {} - unique glyphs ratio {}", subset.name, ratio);
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    fn check_high_priority(&mut self, name: &'static str) -> Result<()> {
        debug!("Checking high priority subset: {name}");
        let subset = self.data.by_name.get(name).unwrap();
        if self.unique_available_ratio(subset) > 0.25 {
            self.do_subset(subset)?;
        }
        Ok(())
    }

    fn split_residual(&mut self) -> Result<()> {
        let glyphs = self.font.all_glyphs() - &self.fulfilled_glyphs;
        if !glyphs.is_empty() {
            info!("Splitting subset from font: residual (unique glyphs: {})", glyphs.len());
            let subset_woff2 = self.font.subset(&glyphs)?;
            self.woff2_subsets.insert("residual", subset_woff2);
        }
        Ok(())
    }
}

pub fn split_webfont(data: &'static WebfontDataCtx, path: PathBuf) -> Result<()> {
    let buffer = std::fs::read(&path)?;

    let mut ctx = FontSplittingContext::new(data, &buffer)?;
    for subset in HIGH_PRIORITY {
        ctx.check_high_priority(subset)?;
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
