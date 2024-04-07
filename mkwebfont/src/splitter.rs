use crate::gf_ranges::{GfSubset, GfSubsets};
use allsorts::{binary::read::ReadScope, font_data::FontData, tables::FontTableProvider};
use anyhow::*;
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::Write,
    path::PathBuf,
    rc::Rc,
};
use tracing::{debug, info};

const HIGH_PRIORITY: &[&str] = &["latin", "latin-ext"];

struct FontSplittingContext<T: FontTableProvider> {
    font: T,
    fulfilled_glyphs: HashSet<char>,
    subsets: HashMap<&'static str, Vec<u8>>,
    present_glyphs_cache: HashMap<&'static str, Rc<HashSet<char>>>,
    subset_glyphs_cache: HashMap<&'static str, Rc<HashSet<char>>>,
    subset_cache: HashMap<&'static str, &'static GfSubset>,
    glyphs_in_font: HashSet<char>,
}
impl<T: FontTableProvider> FontSplittingContext<T> {
    fn new(font: T) -> Result<Self> {
        let mut subset_cache = HashMap::new();
        for subset in GfSubsets::DATA.subsets {
            subset_cache.insert(subset.name, subset);
        }
        let glyphs_in_font = crate::subset::glyphs_in_font(&font)?;
        Ok(FontSplittingContext {
            font,
            fulfilled_glyphs: Default::default(),
            subsets: Default::default(),
            present_glyphs_cache: Default::default(),
            subset_glyphs_cache: Default::default(),
            subset_cache,
            glyphs_in_font,
        })
    }

    fn do_subset(&mut self, subset: &'static GfSubset) -> Result<()> {
        if !self.subsets.contains_key(subset.name) {
            let glyphs = self.present_glyphs(subset)?;
            let subset_otf = crate::subset::subset(&self.font, subset.ranges)?;

            let mut new_glyphs = (*glyphs).clone();
            for gylph in &self.fulfilled_glyphs {
                new_glyphs.remove(gylph);
            }
            info!(
                "Splitting subset from font: {} (unique glyphs: {})",
                subset.name,
                new_glyphs.len()
            );

            self.fulfilled_glyphs.extend(glyphs.iter());
            self.subsets.insert(subset.name, subset_otf);
        }
        Ok(())
    }

    fn present_glyphs(&mut self, subset: &'static GfSubset) -> Result<Rc<HashSet<char>>> {
        if let Some(subset) = self.present_glyphs_cache.get(subset.name) {
            Ok(subset.clone())
        } else {
            let data = crate::subset::glyphs_in_font_subset(&self.font, subset.ranges)?;
            self.present_glyphs_cache.insert(subset.name, Rc::new(data));
            self.present_glyphs(subset)
        }
    }
    fn subset_glyphs(&mut self, subset: &'static GfSubset) -> Result<Rc<HashSet<char>>> {
        if let Some(subset) = self.subset_glyphs_cache.get(subset.name) {
            Ok(subset.clone())
        } else {
            let mut data = HashSet::new();
            for range in subset.ranges {
                for char in range.clone() {
                    data.insert(char);
                }
            }
            self.subset_glyphs_cache.insert(subset.name, Rc::new(data));
            self.subset_glyphs(subset)
        }
    }
    fn unique_available_ratio(&mut self, subset: &'static GfSubset) -> Result<f64> {
        let mut present_glyphs = (*self.present_glyphs(subset)?).clone();
        let mut subset_glyphs = (*self.subset_glyphs(subset)?).clone();
        for glyph in &self.fulfilled_glyphs {
            present_glyphs.remove(glyph);
            subset_glyphs.remove(glyph);
        }
        Ok((present_glyphs.len() as f64) / (subset_glyphs.len() as f64))
    }
    fn unique_available_count(&mut self, subset: &'static GfSubset) -> Result<usize> {
        let mut present_glyphs = (*self.present_glyphs(subset)?).clone();
        for glyph in &self.fulfilled_glyphs {
            present_glyphs.remove(glyph);
        }
        Ok(present_glyphs.len())
    }

    fn select_next_subset(&mut self) -> Result<Option<&'static GfSubset>> {
        let mut selected = None;
        for v in GfSubsets::DATA.subsets {
            if !self.subsets.contains_key(v.name) {
                let count = self.unique_available_count(v)?;
                if count != 0 {
                    let ratio = self.unique_available_ratio(v)?;
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
        let subset = *self.subset_cache.get(name).unwrap();
        if self.unique_available_ratio(subset)? > 0.25 {
            self.do_subset(subset)?;
        }
        Ok(())
    }

    fn split_residual(&mut self) -> Result<()> {
        // TODO: Optimize to remove this weird range hack.
        let mut glyphs = self.glyphs_in_font.clone();
        for glyph in &self.fulfilled_glyphs {
            glyphs.remove(glyph);
        }

        if !glyphs.is_empty() {
            let mut glyph_ranges = Vec::new();
            for glyph in glyphs {
                glyph_ranges.push(glyph..=glyph);
            }

            info!("Splitting subset from font: residual (unique glyphs: {})", glyph_ranges.len());
            let subset_otf = crate::subset::subset(&self.font, &glyph_ranges)?;
            self.subsets.insert("residual", subset_otf);
        }

        Ok(())
    }
}

pub fn test(path: PathBuf) -> Result<()> {
    let buffer = std::fs::read(&path)?;
    let font_file = ReadScope::new(&buffer).read::<FontData>()?;
    let provider = font_file.table_provider(0)?;

    let mut ctx = FontSplittingContext::new(provider)?;
    for subset in HIGH_PRIORITY {
        ctx.check_high_priority(subset)?;
    }
    while let Some(subset) = ctx.select_next_subset()? {
        ctx.do_subset(subset)?;
    }
    ctx.split_residual()?;

    for (k, v) in &ctx.subsets {
        let mut file = File::create(format!("run/{k}.woff2"))?;
        file.write_all(v.as_slice())?;
    }

    Ok(())
}
