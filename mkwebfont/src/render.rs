use crate::{
    contrib::nix_base32,
    fonts::{FontStyle, FontWeight, LoadedFont},
    subset_manifest::{WebfontSubset, WebfontSubsetGroup},
    WebfontCtx, WebfontCtxData,
};
use anyhow::*;
use roaring::RoaringBitmap;
use serde::Deserialize;
use std::{
    collections::HashSet,
    fmt::{Display, Formatter},
    fs,
    ops::RangeInclusive,
    path::Path,
    sync::Arc,
};
use tokio::{task, task::JoinHandle};
use tracing::{debug, info, Instrument};
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

fn extract_name(str: &str) -> String {
    let mut out = String::new();
    for char in str.chars() {
        if char.is_alphanumeric() {
            out.push(char);
        }
        if out.len() == 20 {
            break;
        }
    }
    out
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

/// Contains the data needed to use a font as a webfont.
#[derive(Debug)]
pub struct WebfontInfo {
    font_family: String,
    font_style: FontStyle,
    font_weight: FontWeight,
    entries: Vec<SubsetInfo>,
}
impl WebfontInfo {
    /// Writes the webfont files to the given directory.
    pub fn write_to_store(&self, target: &Path) -> Result<()> {
        let mut path = target.to_path_buf();
        for entry in &self.entries {
            path.push(&entry.file_name);
            debug!("Writing {}...", path.display());
            fs::write(&path, &entry.woff2_data)?;
            path.pop();
        }
        Ok(())
    }

    /// Returns a stylesheet appropriate for using this webfont.
    pub fn render_css<'a>(&'a self, store_uri: &str) -> impl Display + 'a {
        FontStylesheetDisplay { store_uri: store_uri.to_string(), sheet: self }
    }

    /// Returns the number of subsets in the webfont.
    pub fn subset_count(&self) -> usize {
        self.entries.len()
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

#[derive(Debug)]
struct SubsetInfo {
    file_name: String,
    subset_ranges: Vec<RangeInclusive<char>>,
    woff2_data: Vec<u8>,
}
impl SubsetInfo {
    fn new(font: &LoadedFont, name: &str, subset: RoaringBitmap, woff2_data: Vec<u8>) -> Self {
        let blake3_hash = blake3::hash(&woff2_data);
        let hash_str = nix_base32::to_nix_base32(&*blake3_hash.as_bytes());
        let hash_str = &hash_str[1..21];

        let font_name = extract_name(font.font_family());
        let font_style = extract_name(font.font_style());
        let font_version = extract_version(font.font_version());
        let is_regular = font_style.to_lowercase() == "regular";

        let subset_ranges = crate::subset_manifest::decode_range(&subset);

        SubsetInfo {
            file_name: format!(
                "{font_name}{}{}_{font_version}_{name}_{hash_str}.woff2",
                if !is_regular || font.is_variable() { "_" } else { "" },
                if font.is_variable() {
                    "Variable"
                } else if !is_regular {
                    &font_style
                } else {
                    ""
                },
            ),
            subset_ranges,
            woff2_data,
        }
    }
}

struct FontStylesheetDisplay<'a> {
    pub store_uri: String,
    pub sheet: &'a WebfontInfo,
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
            writeln!(f, "    unicode-range: {};", UnicodeRange(&entry.subset_ranges))?;
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

pub struct FontEncoder {
    font: LoadedFont,
    woff2_subsets: Vec<JoinHandle<Result<SubsetInfo>>>,
}
impl FontEncoder {
    fn new(font: LoadedFont) -> Self {
        FontEncoder { font, woff2_subsets: Vec::new() }
    }

    pub fn add_subset(&mut self, name: &str, codepoints: RoaringBitmap) {
        let name = name.to_string();
        let font = self.font.clone();
        self.woff2_subsets.push(task::spawn(
            async move {
                debug!("Encoding subset '{name}' with {} codepoints.", codepoints.len());
                let subset_woff2 = font.subset(&name, &codepoints)?;
                Ok(SubsetInfo::new(&font, &name, codepoints, subset_woff2))
            }
            .in_current_span(),
        ));
    }

    pub async fn produce_webfont(self) -> Result<WebfontInfo> {
        let mut entries = Vec::new();
        for data in self.woff2_subsets {
            entries.push(data.await??);
        }
        entries.sort_by_cached_key(|x| x.file_name.to_string());

        Ok(WebfontInfo {
            font_family: self.font.font_family().to_string(),
            font_style: self.font.parsed_font_style(),
            font_weight: self.font.parsed_font_weight(),
            entries,
        })
    }
}

struct FontSplittingContext {
    ctx: Arc<WebfontCtxData>,
    font: LoadedFont,
    encoder: FontEncoder,
    fulfilled_codepoints: RoaringBitmap,
    processed_subsets: HashSet<Arc<str>>,
    processed_groups: HashSet<Arc<str>>,
    residual_id: usize,
    preload_done: bool,
}
impl FontSplittingContext {
    fn new(ctx: &WebfontCtx, font: &LoadedFont) -> Result<Self> {
        Ok(FontSplittingContext {
            ctx: ctx.0.clone(),
            font: font.clone(),
            encoder: FontEncoder::new(font.clone()),
            fulfilled_codepoints: Default::default(),
            processed_subsets: Default::default(),
            processed_groups: Default::default(),
            residual_id: 0,
            preload_done: false,
        })
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

    fn select_subset_group(&mut self) -> Result<Option<Arc<WebfontSubsetGroup>>> {
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
                Ok(Some(subset.clone()))
            } else {
                debug!("Omitted subset group {} - unique codepoints ratio {}", subset.name, ratio);
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }
    fn select_next_subset(&mut self) -> Result<Option<Arc<WebfontSubset>>> {
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
                return Ok(Some((*subset).clone()));
            }
        }
        for (subset, count, ratio) in top {
            debug!("Omitted subset {} - unique count {count} - unique ratio {ratio}", subset.name);
        }
        Ok(None)
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

/// The internal function that actually splits the webfont.
pub async fn split_webfont(split_ctx: &WebfontCtx, font: &LoadedFont) -> Result<WebfontInfo> {
    debug!("Beginning splitting fonts...");

    let mut ctx = FontSplittingContext::new(split_ctx, font)?;
    ctx.check_high_priority();
    while let Some(subset_group) = ctx.select_subset_group()? {
        ctx.do_subset_group(&subset_group);
    }
    while let Some(subset) = ctx.select_next_subset()? {
        ctx.do_subset(&subset);
    }
    ctx.split_residual();

    debug!("Control process done. Awaiting font data...");

    let info = ctx.encoder.produce_webfont().await?;
    info!(
        "Successfully split {} codepoints into {} subsets!",
        font.all_codepoints().len(),
        info.entries.len(),
    );
    Ok(info)
}
