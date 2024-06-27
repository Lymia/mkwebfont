use crate::font_info::{FontFaceWrapper, FontStyle, FontWeight};
use anyhow::*;
use mkwebfont_common::hashing::{hash_fragment, hash_full};
use roaring::RoaringBitmap;
use std::{fs, ops::RangeInclusive, path::Path, sync::Arc};
use tokio::{task, task::JoinHandle};
use tracing::{debug, Instrument};
use unicode_blocks::find_unicode_block;

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

fn is_same_block(ch_a: char, ch_b: char) -> bool {
    if let Some(block_a) = find_unicode_block(ch_a) {
        if let Some(block_b) = find_unicode_block(ch_b) {
            return block_a.name() == block_b.name();
        }
    }
    false
}

fn decode_range(bitmap: &RoaringBitmap, all_chars: &RoaringBitmap) -> Vec<RangeInclusive<u32>> {
    let mut range_start = None;
    let mut range_last = '\u{fffff}';
    let mut ranges = Vec::new();

    for ch in bitmap {
        let ch = char::from_u32(ch).expect("Invalid char in RoaringBitmap");
        if let Some(start) = range_start {
            let next = char::from_u32(range_last as u32 + 1).unwrap();
            if next != ch {
                let mut can_merge = false;
                if is_same_block(next, ch) {
                    can_merge = true;
                    for ch in next..ch {
                        if all_chars.contains(ch as u32) {
                            can_merge = false;
                            break;
                        }
                    }
                }

                if !can_merge {
                    ranges.push(start as u32..=range_last as u32);
                    range_start = Some(ch);
                }
            }
        } else {
            range_start = Some(ch);
        }
        range_last = ch;
    }
    if let Some(start) = range_start {
        ranges.push(start as u32..=range_last as u32);
    }

    ranges
}

/// Contains the data needed to use a font as a webfont.
#[derive(Debug, Clone)]
pub struct WebfontInfo {
    font_family: Arc<str>,
    font_style_text: Arc<str>,
    font_style: FontStyle,
    font_weight: FontWeight,
    weight_range: RangeInclusive<u32>,
    entries: Vec<Arc<SubsetInfo>>,
}
impl WebfontInfo {
    /// Writes the webfont files to the given directory.
    pub fn write_to_store(&self, target: &Path) -> Result<()> {
        let mut path = target.to_path_buf();
        for entry in &self.entries {
            path.push(&entry.woff2_file_name);
            debug!("Writing {}...", path.display());
            fs::write(&path, &entry.woff2_data)?;
            path.pop();
        }
        Ok(())
    }

    pub fn font_family(&self) -> &str {
        &self.font_family
    }

    pub fn font_style(&self) -> &str {
        &self.font_style_text
    }

    pub fn parsed_font_style(&self) -> FontStyle {
        self.font_style
    }

    pub fn parsed_font_weight(&self) -> FontWeight {
        self.font_weight
    }

    pub fn weight_range(&self) -> RangeInclusive<u32> {
        self.weight_range.clone()
    }

    /// Returns the number of subsets in the webfont.
    pub fn subset_count(&self) -> usize {
        self.entries.len()
    }

    /// Returns the subsets in this webfont.
    pub fn subsets(&self) -> &[Arc<SubsetInfo>] {
        &self.entries
    }

    /// Returns the bitset of characters in the webfont.
    pub fn all_chars(&self) -> RoaringBitmap {
        let mut bitmap = RoaringBitmap::new();
        for subset in &self.entries {
            bitmap.extend(&subset.subset);
        }
        bitmap
    }
}

#[derive(Debug, Clone)]
pub struct SubsetInfo {
    name: String,
    woff2_file_name: String,
    subset: RoaringBitmap,
    subset_ranges: Vec<RangeInclusive<u32>>,
    woff2_data: Vec<u8>,
}
impl SubsetInfo {
    fn new(font: &FontFaceWrapper, name: &str, subset: RoaringBitmap, woff2_data: Vec<u8>) -> Self {
        let font_name = extract_name(font.font_family());
        let font_style = extract_name(font.font_style());
        let font_version = extract_version(font.font_version());
        let is_regular = font_style.to_lowercase() == "regular";

        let subset_ranges = decode_range(&subset, &font.all_codepoints());

        SubsetInfo {
            name: name.to_string(),
            woff2_file_name: format!(
                "{font_name}{}{}_{font_version}_{name}",
                if !is_regular || font.is_variable() { "_" } else { "" },
                if font.is_variable() {
                    "Variable"
                } else if !is_regular {
                    &font_style
                } else {
                    ""
                },
            ),
            subset,
            subset_ranges,
            woff2_data,
        }
    }

    fn finalize_name(&mut self, frag: &str) {
        self.woff2_file_name = format!("{}_{frag}.woff2", self.woff2_file_name);
    }

    /// Returns the name of the subset.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the file name that this subset will be saved to.
    pub fn woff2_file_name(&self) -> &str {
        &self.woff2_file_name
    }

    /// Returns the characters this subset applies to.
    pub fn subset(&self) -> &RoaringBitmap {
        &self.subset
    }

    /// Returns the unicode ranges this subset covers.
    pub fn unicode_ranges(&self) -> &[RangeInclusive<u32>] {
        &self.subset_ranges
    }

    /// Returns the .woff2 data as an array.
    pub fn woff2_data(&self) -> &[u8] {
        &self.woff2_data
    }
}

pub struct FontEncoder {
    font: FontFaceWrapper,
    woff2_subsets: Vec<JoinHandle<Result<SubsetInfo>>>,
}
impl FontEncoder {
    pub fn new(font: FontFaceWrapper) -> Self {
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
        entries.sort_by_cached_key(|x| x.woff2_file_name.to_string());

        let fragment = {
            let mut data = Vec::new();
            for entry in &entries {
                data.extend(hash_full(&entry.woff2_data).as_bytes());
            }
            hash_fragment(&data)
        };
        let entries: Vec<_> = entries
            .into_iter()
            .map(|mut x| {
                x.finalize_name(&fragment);
                Arc::new(x)
            })
            .collect();

        Ok(WebfontInfo {
            font_family: self.font.font_family().to_string().into(),
            font_style_text: self.font.font_style().to_string().into(),
            font_style: self.font.parsed_font_style(),
            font_weight: self.font.parsed_font_weight(),
            weight_range: self.font.weight_range(),
            entries,
        })
    }
}
