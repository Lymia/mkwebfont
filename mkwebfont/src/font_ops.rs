//! Code from <https://github.com/yeslogic/allsorts-tools/blob/master/src/subset.rs>

use crate::woff2;
use allsorts::{
    binary::read::ReadScope,
    font::read_cmap_subtable,
    font_data::FontData,
    tables::{cmap::Cmap, FontTableProvider},
    tag,
};
use anyhow::*;
use roaring::RoaringBitmap;
use std::{
    ffi::CString,
    fmt::{Display, Formatter},
};
use tracing::debug;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum FontStyle {
    Regular,
    Italic,
    Oblique,
}
impl FontStyle {
    fn infer_from_style(style: &str) -> FontStyle {
        let style = style.to_lowercase().replace("-", " ");
        match style {
            x if x.contains("regular") => FontStyle::Regular,
            x if x.contains("italic") => FontStyle::Italic,
            x if x.contains("oblique") => FontStyle::Oblique,
            _ => FontStyle::Regular,
        }
    }
}
impl Display for FontStyle {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            FontStyle::Regular => f.write_str("normal"),
            FontStyle::Italic => f.write_str("italic"),
            FontStyle::Oblique => f.write_str("oblique"),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum FontWeight {
    Regular,
    Bold,
    Numeric(u32),
}
impl FontWeight {
    pub fn infer_from_style(style: &str) -> FontWeight {
        let style = style.to_lowercase().replace("-", " ");
        match style {
            x if x.contains("regular") => FontWeight::Regular,
            x if x.contains("thin") || x.contains("hairline") => FontWeight::Numeric(100),
            x if x.contains("extralight") || x.contains("extra light") => FontWeight::Numeric(200),
            x if x.contains("ultralight") || x.contains("ultra light") => FontWeight::Numeric(200),
            x if x.contains("medium") => FontWeight::Numeric(500),
            x if x.contains("semibold") || x.contains("semi bold") => FontWeight::Numeric(600),
            x if x.contains("demibold") || x.contains("demi bold") => FontWeight::Numeric(600),
            x if x.contains("extrabold") || x.contains("extra bold") => FontWeight::Numeric(800),
            x if x.contains("ultrabold") || x.contains("ultra bold") => FontWeight::Numeric(800),
            x if x.contains("extrablack") || x.contains("extra black") => FontWeight::Numeric(950),
            x if x.contains("ultrablack") || x.contains("ultra black") => FontWeight::Numeric(950),
            x if x.contains("black") => FontWeight::Numeric(900),
            x if x.contains("heavy") => FontWeight::Numeric(900),
            x if x.contains("bold") => FontWeight::Bold,
            _ => FontWeight::Regular,
        }
    }
}
impl Display for FontWeight {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            FontWeight::Regular => f.write_str("normal"),
            FontWeight::Bold => f.write_str("bold"),
            FontWeight::Numeric(n) => write!(f, "{n}"),
        }
    }
}

pub struct LoadedFont {
    pub font_name: String,
    pub font_style: String,
    pub font_version: String,
    pub is_variable: bool,
    pub parsed_font_style: FontStyle,
    pub parsed_font_weight: FontWeight,
    font_data: Vec<u8>,
    available_glyphs: RoaringBitmap,
}
impl LoadedFont {
    pub fn new(buffer: &[u8]) -> Result<LoadedFont> {
        let font_file = ReadScope::new(buffer).read::<FontData>()?;
        let font_provider = font_file.table_provider(0)?;

        let cmap_data = font_provider.read_table_data(tag::CMAP)?;
        let cmap = ReadScope::new(&cmap_data).read::<Cmap>()?;
        let cmap_subtable = read_cmap_subtable(&cmap)?
            .ok_or(Error::msg("no suitable cmap sub-table found"))?
            .1;

        let name_data = font_provider.read_table_data(tag::NAME)?;
        fn cstr_to_str(c: Option<CString>) -> String {
            c.map(|x| x.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string())
        }
        let font_name = cstr_to_str(allsorts::get_name::fontcode_get_name(&name_data, 1)?);
        let font_style = cstr_to_str(allsorts::get_name::fontcode_get_name(&name_data, 2)?);
        let font_version = cstr_to_str(allsorts::get_name::fontcode_get_name(&name_data, 5)?);
        let is_variable = font_provider.has_table(tag::FVAR);
        let parsed_font_style = FontStyle::infer_from_style(&font_style);
        let parsed_font_weight = FontWeight::infer_from_style(&font_style);

        debug!(
            "Loaded font: {font_name} / {font_style} / {font_version}{}",
            if is_variable { " / Variable" } else { "" }
        );
        debug!("Inferred style: {parsed_font_style:?} / {parsed_font_weight:?}");

        let mut available_glyphs = RoaringBitmap::new();
        cmap_subtable.mappings_fn(|x, _| {
            available_glyphs.insert(x);
        })?;

        Ok(LoadedFont {
            font_name,
            font_style,
            font_version,
            is_variable,
            parsed_font_style,
            parsed_font_weight,
            font_data: buffer.to_vec(),
            available_glyphs,
        })
    }

    pub fn glyphs_in_font(&self, set: &RoaringBitmap) -> RoaringBitmap {
        self.available_glyphs.clone() & set
    }

    pub fn all_glyphs(&self) -> &RoaringBitmap {
        &self.available_glyphs
    }

    pub fn subset(&self, chars: &RoaringBitmap) -> Result<Vec<u8>> {
        // Subset the font
        let mut vec = Vec::new();
        for char in chars {
            vec.push(char::from_u32(char).unwrap());
        }
        let new_font = hb_subset::subset(&self.font_data, vec)?;
        Ok(woff2::compress(&new_font, "".to_string(), 9, true).unwrap())
    }
}
