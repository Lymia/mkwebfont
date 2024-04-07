use crate::contrib::woff2;
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

pub struct LoadedFont<'a> {
    pub font_name: String,
    pub font_style: String,
    pub font_version: String,
    pub is_variable: bool,
    pub parsed_font_style: FontStyle,
    pub parsed_font_weight: FontWeight,
    font_face: PreprocessedFontFace<'a>,
    available_codepoints: RoaringBitmap,
}
impl<'a> LoadedFont<'a> {
    fn load_for_font(font_face: FontFace) -> Result<LoadedFont> {
        let is_variable =
            unsafe { hb_subset::sys::hb_ot_var_get_axis_count(font_face.as_raw()) != 0 };

        let font_name = if is_variable {
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
            "Loaded font: {font_name} / {font_style} / {font_version} / {} gylphs{}",
            available_codepoints.len(),
            if is_variable { " / Variable font" } else { "" },
        );
        debug!("Inferred style: {parsed_font_style:?} / {parsed_font_weight:?}");

        Ok(LoadedFont {
            font_name,
            font_style,
            font_version,
            is_variable,
            parsed_font_style,
            parsed_font_weight,
            font_face: font_face.preprocess_for_subsetting(),
            available_codepoints,
        })
    }
    pub fn load(buffer: &[u8]) -> Result<Vec<LoadedFont>> {
        let is_woff = buffer.len() >= 4 && &buffer[0..4] == b"wOFF";
        let is_woff2 = buffer.len() >= 4 && &buffer[0..4] == b"wOF2";
        let is_collection = buffer.len() >= 4 && &buffer[0..4] == b"ttcf";

        if is_woff || is_woff2 {
            bail!("woff/woff2 input is not supported. Please convert to .ttf or .otf first.");
        }

        let blob = Blob::from_bytes(buffer)?;

        let mut fonts = Vec::new();
        fonts.push(Self::load_for_font(FontFace::new_with_index(blob.clone(), 0)?)?);

        if is_collection {
            let mut i = 1;
            while let Result::Ok(font) = FontFace::new_with_index(blob.clone(), i) {
                if font.glyph_count() == 0 {
                    break;
                }
                fonts.push(Self::load_for_font(font)?);
                i += 1;
            }
        }

        debug!("Found {} fonts in collection.", fonts.len());

        Ok(fonts)
    }

    pub fn codepoints_in_fault(&self, set: &RoaringBitmap) -> RoaringBitmap {
        self.available_codepoints.clone() & set
    }

    pub fn all_codepoints(&self) -> &RoaringBitmap {
        &self.available_codepoints
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
