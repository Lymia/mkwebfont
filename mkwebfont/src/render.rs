use crate::{
    contrib::nix_base32,
    fonts::{FontStyle, FontWeight, LoadedFont},
    WebfontCtx,
};
use anyhow::*;
use roaring::RoaringBitmap;
use std::{
    fmt::{Display, Formatter},
    fs,
    ops::RangeInclusive,
    path::Path,
};
use tokio::{task, task::JoinHandle};
use tracing::{debug, info, Instrument};

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

/// The internal function that actually splits the webfont.
pub async fn split_webfont(ctx: &WebfontCtx, font: &LoadedFont) -> Result<WebfontInfo> {
    let mut encoder = FontEncoder::new(font.clone());
    crate::splitter::split_webfonts(ctx, font, &mut encoder);

    let info = encoder.produce_webfont().await?;
    info!(
        "Successfully split {} codepoints into {} subsets!",
        font.all_codepoints().len(),
        info.entries.len(),
    );
    Ok(info)
}
