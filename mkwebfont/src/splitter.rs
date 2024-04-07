use crate::{
    font_ops::{FontStyle, FontWeight, LoadedFont},
    ranges::{WebfontDataCtx, WebfontSubset, WebfontSubsetGroup},
};
use anyhow::*;
use roaring::RoaringBitmap;
use serde::Deserialize;
use std::{
    collections::HashSet,
    fmt::{Display, Formatter},
    fs::File,
    io::Write,
    ops::RangeInclusive,
    path::Path,
};
use tracing::{debug, info};
use unic_ucd_block::Block;

#[derive(Clone, Debug, Deserialize)]
struct TuningParameters {
    reject_subset_threshold: usize,
    accept_subset_count_threshold: usize,
    accept_subset_ratio_threshold: f64,
    accept_group_ratio_threshold: f64,
    high_priority_ratio_threshold: f64,
    high_priority_subsets: Vec<String>,
    residual_class_max_size: usize,
}

fn extract_name(str: &str) -> String {
    let mut out = String::new();
    for char in str.chars() {
        if char.is_alphanumeric() {
            out.push(char);
        } else if out.chars().last() != Some('-') {
            out.push('-');
        }
        if out.len() == 20 {
            break;
        }
    }
    out.trim_matches('-').to_string()
}
fn extract_version(mut str: &str) -> String {
    let mut out = String::new();
    let version_txt = "version ";
    if str.to_lowercase().starts_with(version_txt) {
        str = &str[version_txt.len()..];
    }
    for char in str.chars() {
        if char.is_numeric() || char == '.' {
            out.push(char);
        } else {
            break;
        }
        if out.len() == 20 {
            break;
        }
    }
    out.trim_matches('.').to_string()
}

struct SplitFontData {
    store_file_name: String,
    subset: RoaringBitmap,
    woff2_data: Vec<u8>,
}
impl SplitFontData {
    fn new(font: &LoadedFont, name: &str, subset: RoaringBitmap, woff2_data: Vec<u8>) -> Self {
        let blake3_hash = blake3::hash(&woff2_data);
        let hash_str = crate::nix_base32::to_nix_base32(&*blake3_hash.as_bytes());
        let hash_str = &hash_str[1..21];

        let font_name = extract_name(&font.font_name);
        let font_style = extract_name(&font.font_style);
        let font_version = extract_version(&font.font_version);
        SplitFontData {
            store_file_name: format!(
                "{font_name}_{font_style}_{font_version}_{name}_{hash_str}.woff2"
            ),
            subset,
            woff2_data,
        }
    }
}

struct FontSplittingContext {
    tuning: TuningParameters,
    font: LoadedFont,
    data: &'static WebfontDataCtx,
    fulfilled_glyphs: RoaringBitmap,
    woff2_subsets: Vec<SplitFontData>,
    processed_subsets: HashSet<&'static str>,
    processed_groups: HashSet<&'static str>,
    residual_id: usize,
}
impl FontSplittingContext {
    fn new(tuning: &TuningParameters, data: &'static WebfontDataCtx, font: &[u8]) -> Result<Self> {
        debug!("Font splitting tuning parameters: {tuning:#?}");
        Ok(FontSplittingContext {
            tuning: tuning.clone(),
            font: LoadedFont::new(font)?,
            data,
            fulfilled_glyphs: Default::default(),
            woff2_subsets: Default::default(),
            processed_subsets: Default::default(),
            processed_groups: Default::default(),
            residual_id: 0,
        })
    }

    fn do_subset(&mut self, subset: &'static WebfontSubset) -> Result<()> {
        if !self.processed_subsets.contains(subset.name) {
            self.processed_subsets.insert(subset.name);

            let new_glyphs = self.font.glyphs_in_font(&subset.map) - &self.fulfilled_glyphs;
            if new_glyphs.len() as usize >= self.tuning.reject_subset_threshold {
                let subset_woff2 = self.font.subset(&subset.map)?;
                info!(
                    "Splitting subset from font: {} (unique glyphs: {})",
                    subset.name,
                    new_glyphs.len()
                );
                self.fulfilled_glyphs.extend(new_glyphs.clone());
                self.woff2_subsets.push(SplitFontData::new(
                    &self.font,
                    subset.name,
                    new_glyphs,
                    subset_woff2,
                ));
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
        if subset_glyphs == 0 {
            0.0
        } else {
            (present_glyphs as f64) / (subset_glyphs as f64)
        }
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
        let mut subsets = Vec::new();
        for v in &self.data.subsets {
            if !self.processed_subsets.contains(v.name) {
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
            if *ratio >= self.tuning.accept_subset_ratio_threshold
                || *count >= self.tuning.accept_subset_count_threshold
            {
                return Ok(Some(*subset));
            }
        }
        for (subset, count, ratio) in top {
            debug!(
                "Omitted subset {} - unique glyphs count {count} - unique glyphs ratio {ratio}",
                subset.name
            );
        }
        Ok(None)
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

    fn split_to_blocks(glyphs: RoaringBitmap) -> Vec<RoaringBitmap> {
        let mut last_glyph_block = None;
        let mut accum = RoaringBitmap::new();
        let mut list = Vec::new();
        for glyph in glyphs {
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
    fn generate_residual_block(&mut self, residual: &mut Vec<RoaringBitmap>) -> Result<()> {
        let mut set = RoaringBitmap::new();
        let mut i = 0;
        while i < residual.len() {
            if residual[i].len() as usize > self.tuning.residual_class_max_size && set.is_empty() {
                set = residual[i]
                    .iter()
                    .take(self.tuning.residual_class_max_size)
                    .collect();
                residual[i] = residual[i].clone() - set.clone();
                break;
            } else if (set.len() + residual[i].len()) as usize
                <= self.tuning.residual_class_max_size
            {
                set.extend(residual.remove(i));
            } else {
                i += 1;
            }
        }

        assert!(!set.is_empty());
        let name = format!("residual-s{}", self.residual_id);
        let subset_woff2 = self.font.subset(&set)?;
        info!("Splitting subset from font: {name} (unique glyphs: {})", set.len());
        self.woff2_subsets
            .push(SplitFontData::new(&self.font, &name, set, subset_woff2));
        self.residual_id += 1;

        Ok(())
    }
    fn split_residual(&mut self) -> Result<()> {
        let glyphs = self.font.all_glyphs() - &self.fulfilled_glyphs;
        if !glyphs.is_empty() {
            info!("Splitting residual glyphs into subsets (remaining glyphs: {})", glyphs.len());
            let mut split = Self::split_to_blocks(glyphs);
            while !split.is_empty() {
                self.generate_residual_block(&mut split)?;
            }
        }
        Ok(())
    }
}

struct UnicodeRange<'a>(&'a [RangeInclusive<char>]);
impl<'a> Display for UnicodeRange<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut first = true;
        for range in self.0 {
            if first {
                first = false;
            } else {
                f.write_str(", ")?;
            }

            if range.start() == range.end() {
                write!(f, "U+{:X}", *range.start() as u32)?;
            } else {
                write!(f, "U+{:X}-{:X}", *range.start() as u32, *range.end() as u32)?;
            }
        }
        Result::Ok(())
    }
}

struct FontStylesheetDisplay<'a> {
    pub store_uri: String,
    pub sheet: &'a FontStylesheetInfo,
}
impl<'a> Display for FontStylesheetDisplay<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        for entry in &self.sheet.entries {
            writeln!(f, "@font-face {{")?;
            writeln!(f, "    font-family: {:?};", self.sheet.font_family)?;
            if self.sheet.font_style != FontStyle::Regular {
                writeln!(f, "    font-style: {};", self.sheet.font_style)?;
            }
            if self.sheet.font_weight != FontWeight::Regular {
                writeln!(f, "    font-weight: {};", self.sheet.font_weight)?;
            }
            writeln!(f, "    unicode-range: {};", UnicodeRange(&entry.glyphs))?;
            writeln!(
                f,
                "    src: url({:?}) format(\"woff2\");",
                format!("{}{}", self.store_uri, entry.file_name)
            )?;
            writeln!(f, "}}")?;
        }
        Result::Ok(())
    }
}

#[derive(Debug)]
pub struct FontStylesheetInfo {
    pub font_family: String,
    pub font_style: FontStyle,
    pub font_weight: FontWeight,
    pub entries: Vec<FontStylesheetEntry>,
}
impl FontStylesheetInfo {
    pub fn render_css<'a>(&'a self, store_uri: &str) -> impl Display + 'a {
        FontStylesheetDisplay { store_uri: store_uri.to_string(), sheet: self }
    }
}

#[derive(Debug)]
pub struct FontStylesheetEntry {
    pub file_name: String,
    pub glyphs: Vec<RangeInclusive<char>>,
}

/// The internal function that actually splits the webfont.
pub fn split_webfont(
    tuning: Option<&str>,
    data: &'static WebfontDataCtx,
    font_data: &[u8],
    store_dir: &Path,
) -> Result<FontStylesheetInfo> {
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

    let mut entries = Vec::new();
    for data in &ctx.woff2_subsets {
        let mut target = store_dir.to_path_buf();
        target.push(&data.store_file_name);

        let mut file = File::create(target)?;
        file.write_all(&data.woff2_data)?;

        entries.push(FontStylesheetEntry {
            file_name: data.store_file_name.clone(),
            glyphs: crate::ranges::decode_range(&data.subset),
        });
    }
    entries.sort_by_cached_key(|x| x.file_name.to_string());

    Ok(FontStylesheetInfo {
        font_family: ctx.font.font_name.clone(),
        font_style: ctx.font.parsed_font_style,
        font_weight: ctx.font.parsed_font_weight,
        entries,
    })
}

/// The default tuning parameters for the splitter.
pub const DEFAULT_TUNING_PARAMETERS: &str = include_str!("splitter_default_tuning.toml");
