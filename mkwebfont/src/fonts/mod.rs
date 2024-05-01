use crate::fonts::variation_axises::{AxisName, VariationAxis};
use anyhow::*;
use hb_subset::{Blob, FontFace, SubsetInput};
use roaring::RoaringBitmap;
use std::{
    fmt::{Debug, Display, Formatter},
    sync::Arc,
};
use tracing::debug;
use unicode_properties::{GeneralCategory, UnicodeGeneralCategory};

mod variation_axises;
mod woff2;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum FontStyle {
    Regular,
    Italic,
    Oblique,
}
impl FontStyle {
    fn infer(style: &str) -> FontStyle {
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
    pub fn infer(style: &str) -> FontWeight {
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

#[derive(Clone)]
pub struct LoadedFont(Arc<LoadedFontData>);
struct LoadedFontData {
    font_family: String,
    font_style: String,
    font_version: String,
    variations: Vec<VariationAxis>,
    parsed_font_style: FontStyle,
    parsed_font_weight: FontWeight,
    available_codepoints: RoaringBitmap,
    font_data: Arc<[u8]>,
    font_index: u32,
}
impl LoadedFont {
    pub fn load(buffer: Vec<u8>) -> Result<Vec<LoadedFont>> {
        let is_woff = buffer.len() >= 4 && &buffer[0..4] == b"wOFF";
        let is_woff2 = buffer.len() >= 4 && &buffer[0..4] == b"wOF2";
        let is_collection = buffer.len() >= 4 && &buffer[0..4] == b"ttcf";

        if is_woff || is_woff2 {
            bail!("woff/woff2 input is not supported. Please convert to .ttf or .otf first.");
        }

        let data: Arc<[u8]> = buffer.into();

        let mut fonts = Vec::new();
        if let Some(font) = Self::load_for_font(data.clone(), 0)? {
            fonts.push(font);
        } else {
            bail!("No glyphs in first font?");
        }

        if is_collection {
            let mut i = 1;
            while let Some(x) = Self::load_for_font(data.clone(), i)? {
                fonts.push(x);
                i += 1;
            }
        }

        debug!("Found {} fonts in collection.", fonts.len());

        Ok(fonts)
    }
    fn load_for_font(font_data: Arc<[u8]>, idx: u32) -> Result<Option<LoadedFont>> {
        let blob = Blob::from_bytes(&font_data)?;
        let font_face = FontFace::new_with_index(blob, idx)?;
        if font_face.glyph_count() == 0 {
            return Ok(None);
        }

        let variations = variation_axises::get_variation_axises(&font_face);
        let is_variable = !variations.is_empty();

        let font_family = if is_variable {
            // a lot of dynamic fonts have a weight prebaked in the font_family for some reason
            let family = font_face.font_family();
            let typographic_family = font_face.typographic_family();

            if family.starts_with(&typographic_family) && !typographic_family.is_empty() {
                typographic_family
            } else {
                family
            }
        } else {
            font_face.font_family()
        };
        let font_style = font_face.font_subfamily();
        let font_version = font_face.version_string();
        let parsed_font_style = FontStyle::infer(&font_style);
        let parsed_font_weight = if is_variable {
            FontWeight::Regular // font weight doesn't matter for variable fonts
        } else {
            FontWeight::infer(&font_style)
        };

        let mut available_codepoints = RoaringBitmap::new();
        for char in &font_face.covered_codepoints()? {
            available_codepoints.insert(char as u32);
        }

        debug!(
            "Loaded font: {font_family} / {font_style} / {font_version} / {} gylphs{}",
            available_codepoints.len(),
            if is_variable { " / Variable font" } else { "" },
        );
        debug!("Inferred style: {parsed_font_style:?} / {parsed_font_weight:?}");
        if is_variable {
            if variations.len() == 1 {
                let axis = variations.first().unwrap();
                debug!(
                    "Font axis variations: {} / ({}..={}, default: {})",
                    axis.name,
                    axis.range.start(),
                    axis.range.end(),
                    axis.default,
                );
            } else {
                debug!("Font axis variations: ");
                for axis in &variations {
                    debug!(
                        "- {} / ({}..={}, default: {})",
                        axis.name,
                        axis.range.start(),
                        axis.range.end(),
                        axis.default,
                    );
                }
            }
        }

        drop(font_face);

        Ok(Some(LoadedFont(Arc::new(LoadedFontData {
            font_family,
            font_style,
            font_version,
            variations,
            parsed_font_style,
            parsed_font_weight,
            available_codepoints,
            font_data,
            font_index: idx,
        }))))
    }

    pub fn codepoints_in_set(&self, set: &RoaringBitmap) -> RoaringBitmap {
        self.0.available_codepoints.clone() & set
    }
    pub fn all_codepoints(&self) -> &RoaringBitmap {
        &self.0.available_codepoints
    }
    pub fn font_family(&self) -> &str {
        &self.0.font_family
    }
    pub fn font_style(&self) -> &str {
        &self.0.font_style
    }
    pub fn font_version(&self) -> &str {
        &self.0.font_version
    }
    pub fn is_variable(&self) -> bool {
        !self.0.variations.is_empty()
    }
    pub fn parsed_font_style(&self) -> FontStyle {
        self.0.parsed_font_style
    }
    pub fn parsed_font_weight(&self) -> FontWeight {
        self.0.parsed_font_weight
    }

    pub fn subset(&self, name: &str, chars: &RoaringBitmap) -> Result<Vec<u8>> {
        // Load the font into harfbuzz
        let blob = Blob::from_bytes(&self.0.font_data)?;
        let mut font = FontFace::new_with_index(blob, self.0.font_index)?;

        // Prepare the subsetting plan
        let mut subset_input = SubsetInput::new()?;
        subset_input.unicode_set().clear();
        for ch in chars {
            let ch = char::from_u32(ch).unwrap();
            let cat = ch.general_category();
            if cat != GeneralCategory::Control && cat != GeneralCategory::Format {
                subset_input.unicode_set().insert(ch);
            }
        }
        for variation in &self.0.variations {
            // TODO: Do not hardcode allowed axises
            if variation.is_hidden || variation.axis != Some(AxisName::Weight) {
                variation.pin(&mut font, &mut subset_input);
            }
        }

        // Subset the font
        let new_font = subset_input.subset_font(&font)?;
        let new_font = new_font.underlying_blob().to_vec();
        Ok(woff2::compress(&new_font, name.to_string(), 11, true).unwrap())
    }
}
impl Debug for LoadedFont {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[font: {} / {} / {}]",
            self.font_family(),
            self.font_style(),
            self.font_version(),
        )
    }
}
impl Display for LoadedFont {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.font_family(), self.font_style())
    }
}
