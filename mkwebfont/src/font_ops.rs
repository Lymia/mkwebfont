use crate::woff2;
use anyhow::*;
use hb_subset::{Blob, FontFace, PreprocessedFontFace, SubsetInput};
use roaring::RoaringBitmap;
use std::fmt::{Display, Formatter};
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

pub struct LoadedFont<'a> {
    pub font_name: String,
    pub font_style: String,
    pub font_version: String,
    pub parsed_font_style: FontStyle,
    pub parsed_font_weight: FontWeight,
    font_face: PreprocessedFontFace<'a>,
    available_glyphs: RoaringBitmap,
}
impl<'a> LoadedFont<'a> {
    pub fn new(buffer: &[u8]) -> Result<LoadedFont> {
        let font_face = FontFace::new(Blob::from_bytes(buffer)?)?;

        let font_name = font_face.font_family();
        let font_style = font_face.font_subfamily();
        let font_version = font_face.version_string();
        let parsed_font_style = FontStyle::infer_from_style(&font_style);
        let parsed_font_weight = FontWeight::infer_from_style(&font_style);

        let mut available_glyphs = RoaringBitmap::new();
        for char in &font_face.covered_codepoints()? {
            available_glyphs.insert(char as u32);
        }

        debug!(
            "Loaded font: {font_name} / {font_style} / {font_version} / {} gylphs",
            available_glyphs.len()
        );
        debug!("Inferred style: {parsed_font_style:?} / {parsed_font_weight:?}");

        Ok(LoadedFont {
            font_name,
            font_style,
            font_version,
            parsed_font_style,
            parsed_font_weight,
            font_face: font_face.preprocess_for_subsetting(),
            available_glyphs,
        })
    }

    pub fn glyphs_in_font(&self, set: &RoaringBitmap) -> RoaringBitmap {
        self.available_glyphs.clone() & set
    }

    pub fn all_glyphs(&self) -> &RoaringBitmap {
        &self.available_glyphs
    }

    pub fn subset(&self, name: &str, chars: &RoaringBitmap) -> Result<Vec<u8>> {
        // Subset the font
        let mut subset_input = SubsetInput::new()?;
        subset_input.unicode_set().clear();
        for char in chars {
            subset_input
                .unicode_set()
                .insert(char::from_u32(char).unwrap());
        }

        let new_font = subset_input.subset_font(&self.font_face)?;
        let new_font = new_font.underlying_blob().to_vec();
        Ok(woff2::compress(&new_font, name.to_string(), 9, true).unwrap())
    }
}
