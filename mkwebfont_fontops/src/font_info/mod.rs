use anyhow::{bail, Result};
use bincode::{Decode, Encode};
use enumset::{EnumSet, EnumSetType};
use hb_subset::{Blob, FontFace, SubsetInput};
use mkwebfont_common::{character_set::CharacterSet, hashing::WyHashBuilder};
use std::{
    collections::{HashMap, HashSet},
    fmt::{Debug, Display, Formatter},
    ops::RangeInclusive,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tracing::debug;

mod variation_axises;
mod woff2;

pub use variation_axises::{AxisName, VariationAxis};

#[derive(EnumSetType, Debug, Decode, Encode)]
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

    pub(crate) fn is_compatible(&self, other: FontStyle) -> bool {
        match (*self, other) {
            (Self::Regular, _) => true,
            (Self::Oblique | Self::Italic, Self::Oblique | Self::Italic) => true,
            _ => false,
        }
    }
}
impl Display for FontStyle {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            FontStyle::Regular => f.write_str("Normal"),
            FontStyle::Italic => f.write_str("Italic"),
            FontStyle::Oblique => f.write_str("Oblique"),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum FontWeight {
    Regular,
    Bold,
    Numeric(u32),
}
impl FontWeight {
    pub fn infer(style: &str) -> FontWeight {
        let style = style.to_lowercase().replace("-", " ");
        match style {
            x if x.contains("thin") || x.contains("hairline") => FontWeight::Numeric(100),
            x if x.contains("extralight") || x.contains("extra light") => FontWeight::Numeric(200),
            x if x.contains("ultralight") || x.contains("ultra light") => FontWeight::Numeric(200),
            x if x.contains("light") => FontWeight::Numeric(300),
            x if x.contains("regular") => FontWeight::Regular,
            x if x.contains("medium") => FontWeight::Numeric(500),
            x if x.contains("semibold") || x.contains("semi bold") => FontWeight::Numeric(600),
            x if x.contains("demibold") || x.contains("demi bold") => FontWeight::Numeric(600),
            x if x.contains("bold") => FontWeight::Bold,
            x if x.contains("extrabold") || x.contains("extra bold") => FontWeight::Numeric(800),
            x if x.contains("ultrabold") || x.contains("ultra bold") => FontWeight::Numeric(800),
            x if x.contains("black") => FontWeight::Numeric(900),
            x if x.contains("heavy") => FontWeight::Numeric(900),
            x if x.contains("extrablack") || x.contains("extra black") => FontWeight::Numeric(950),
            x if x.contains("ultrablack") || x.contains("ultra black") => FontWeight::Numeric(950),
            _ => FontWeight::Regular,
        }
    }

    pub fn from_num(num: u32) -> Self {
        match num {
            400 => FontWeight::Regular,
            700 => FontWeight::Bold,
            _ => FontWeight::Numeric(num),
        }
    }

    pub fn as_num(&self) -> u32 {
        match self {
            FontWeight::Regular => 400,
            FontWeight::Bold => 700,
            FontWeight::Numeric(n) => *n,
        }
    }

    pub(crate) fn dist_from_range(&self, range: &RangeInclusive<u32>) -> u32 {
        let num = self.as_num();
        if range.contains(&num) {
            0
        } else {
            range.start().abs_diff(num).min(range.end().abs_diff(num))
        }
    }
}
impl Display for FontWeight {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            FontWeight::Numeric(100) => f.write_str("Thin"),
            FontWeight::Numeric(200) => f.write_str("Extra Light"),
            FontWeight::Numeric(300) => f.write_str("Light"),
            FontWeight::Numeric(400) | FontWeight::Regular => f.write_str("Regular"),
            FontWeight::Numeric(500) => f.write_str("Medium"),
            FontWeight::Numeric(600) => f.write_str("Semi-Bold"),
            FontWeight::Numeric(700) | FontWeight::Bold => f.write_str("Bold"),
            FontWeight::Numeric(800) => f.write_str("Extra-Bold"),
            FontWeight::Numeric(900) => f.write_str("Black"),
            FontWeight::Numeric(950) => f.write_str("Extra-Black"),
            FontWeight::Numeric(n) => write!(f, "{n}"),
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct FontId(usize);
impl FontId {
    fn new() -> Self {
        static CURRENT_ID: AtomicUsize = AtomicUsize::new(0);
        loop {
            let cur = CURRENT_ID.load(Ordering::Relaxed);
            assert_ne!(cur, usize::MAX);
            if CURRENT_ID
                .compare_exchange(cur, cur + 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return FontId(cur);
            }
        }
    }
}

#[derive(Clone)]
pub struct FontFaceWrapper(Arc<FontFaceData>);
struct FontFaceData {
    font_id: FontId,
    font_family: String,
    font_style: String,
    font_version: String,
    variations: Vec<VariationAxis>,
    parsed_font_style: FontStyle,
    parsed_font_weight: FontWeight,
    available_codepoints: CharacterSet,
    font_data: Arc<[u8]>,
    font_index: u32,
    filename_hint: Option<String>,
}
impl FontFaceWrapper {
    pub fn load(
        filename_hint: Option<String>,
        buffer: impl Into<Arc<[u8]>>,
    ) -> Result<Vec<FontFaceWrapper>> {
        let buffer: Arc<[u8]> = buffer.into();

        let is_woff = buffer.len() >= 4 && &buffer[0..4] == b"wOFF";
        let is_woff2 = buffer.len() >= 4 && &buffer[0..4] == b"wOF2";
        let is_collection = buffer.len() >= 4 && &buffer[0..4] == b"ttcf";

        if is_woff || is_woff2 {
            bail!("woff/woff2 input is not supported. Please convert to .ttf or .otf first.");
        }

        let mut fonts = Vec::new();
        if let Some(font) = Self::load_for_font(filename_hint.clone(), buffer.clone(), 0)? {
            fonts.push(font);
        } else {
            bail!("No glyphs in first font?");
        }

        if is_collection {
            let mut i = 1;
            while let Some(x) = Self::load_for_font(filename_hint.clone(), buffer.clone(), i)? {
                fonts.push(x);
                i += 1;
            }
        }

        debug!("Found {} fonts in collection.", fonts.len());

        Ok(fonts)
    }
    fn load_for_font(
        filename_hint: Option<String>,
        font_data: Arc<[u8]>,
        idx: u32,
    ) -> Result<Option<FontFaceWrapper>> {
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
        let font_version = font_face
            .version_string()
            .split(';')
            .next()
            .unwrap()
            .trim()
            .to_string();
        let parsed_font_style = FontStyle::infer(&font_style);
        let parsed_font_weight = if is_variable {
            FontWeight::Regular // font weight doesn't matter for variable fonts
        } else {
            FontWeight::infer(&font_style)
        };

        let mut available_codepoints = CharacterSet::new();
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

        Ok(Some(FontFaceWrapper(Arc::new(FontFaceData {
            font_id: FontId::new(),
            font_family,
            font_style,
            font_version,
            variations,
            parsed_font_style,
            parsed_font_weight,
            available_codepoints,
            font_data,
            font_index: idx,
            filename_hint,
        }))))
    }

    pub fn codepoints_in_set(&self, set: &CharacterSet) -> CharacterSet {
        self.0.available_codepoints.clone() & set
    }
    pub fn all_codepoints(&self) -> &CharacterSet {
        &self.0.available_codepoints
    }
    pub fn font_id(&self) -> FontId {
        self.0.font_id
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
    pub fn variations(&self) -> &[VariationAxis] {
        &self.0.variations
    }
    pub fn parsed_font_style(&self) -> FontStyle {
        self.0.parsed_font_style
    }
    pub fn parsed_font_weight(&self) -> FontWeight {
        self.0.parsed_font_weight
    }

    pub fn font_data(&self) -> &[u8] {
        &self.0.font_data
    }

    pub fn weight_range(&self) -> RangeInclusive<u32> {
        if let Some(axis) = self
            .variations()
            .iter()
            .find(|x| x.axis == Some(AxisName::Weight))
        {
            *axis.range.start() as u32..=*axis.range.end() as u32
        } else {
            let weight = self.parsed_font_weight().as_num();
            weight..=weight
        }
    }

    pub fn subset(&self, name: &str, chars: &CharacterSet) -> Result<Vec<u8>> {
        // Load the font into harfbuzz
        let blob = Blob::from_bytes(&self.0.font_data)?;
        let mut font = FontFace::new_with_index(blob, self.0.font_index)?;

        // Prepare the subsetting plan
        let mut subset_input = SubsetInput::new()?;
        subset_input.unicode_set().clear();
        for ch in chars {
            let ch = char::from_u32(ch).unwrap();
            subset_input.unicode_set().insert(ch);
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
impl Debug for FontFaceWrapper {
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
impl Display for FontFaceWrapper {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.font_family(), self.font_style())
    }
}

#[derive(Clone, Debug)]
pub struct FontFaceSet {
    list: Vec<FontFaceWrapper>,
    by_name: HashMap<String, Vec<FontFaceWrapper>, WyHashBuilder>,
    by_id: HashMap<FontId, FontFaceWrapper, WyHashBuilder>,
}
impl FontFaceSet {
    fn push_name(&mut self, name: &str, font: &FontFaceWrapper) {
        let lc_name = name.to_lowercase();
        self.by_name.entry(lc_name).or_default().push(font.clone());
    }

    pub fn build(fonts: impl Iterator<Item = FontFaceWrapper>) -> FontFaceSet {
        let mut set =
            FontFaceSet { list: vec![], by_name: Default::default(), by_id: Default::default() };

        for font in fonts {
            set.list.push(font.clone());
            if let Some(filename_hint) = &font.0.filename_hint {
                set.push_name(filename_hint.as_str(), &font);
            }
            set.push_name(font.font_family(), &font);
            set.push_name(&format!("{} {}", font.font_family(), font.font_style()), &font);
            set.by_id.insert(font.font_id(), font);
        }

        set
    }

    pub fn as_list(&self) -> &[FontFaceWrapper] {
        &self.list
    }

    pub fn get_by_id(&self, id: FontId) -> Option<&FontFaceWrapper> {
        self.by_id.get(&id)
    }

    pub fn resolve(&self, name: &str) -> Result<&FontFaceWrapper> {
        let resolve = self.resolve_all(name)?;
        if resolve.len() == 1 {
            Ok(&resolve[0])
        } else {
            bail!("Font name {name:?} is ambigious!");
        }
    }

    pub fn resolve_all(&self, name: &str) -> Result<&[FontFaceWrapper]> {
        let lc_name = name.to_lowercase();
        if let Some(list) = self.by_name.get(&lc_name) {
            Ok(list.as_slice())
        } else {
            bail!("Font name {name:?} does not exist.");
        }
    }

    pub fn resolve_by_style(
        &self,
        name: &str,
        style: FontStyle,
        weight: FontWeight,
    ) -> Result<&FontFaceWrapper> {
        let mut near_match = Vec::new();
        for font in &self.list {
            if font.font_family().eq_ignore_ascii_case(name)
                && font.parsed_font_style().is_compatible(style)
            {
                if font.parsed_font_style() == style
                    && font.weight_range().contains(&weight.as_num())
                {
                    return Ok(font);
                } else {
                    near_match.push(font);
                }
            }
        }
        if let Some(font) = near_match
            .iter()
            .map(|x| {
                (x, (x.parsed_font_style() == style, weight.dist_from_range(&x.weight_range())))
            })
            .max_by_key(|x| x.1)
            .map(|x| *x.0)
        {
            Ok(font)
        } else {
            bail!("No fonts match specification: {name} / {style} / {weight}");
        }
    }

    pub fn resolve_by_styles(
        &self,
        name: &str,
        styles: EnumSet<FontStyle>,
        weights: &[FontWeight],
    ) -> Result<Vec<&FontFaceWrapper>> {
        let mut ids: HashSet<_, WyHashBuilder> = HashSet::default();
        for style in styles {
            for weight in weights {
                ids.insert(self.resolve_by_style(name, style, *weight)?.font_id());
            }
        }

        let mut ids: Vec<_> = ids.into_iter().collect();
        ids.sort();
        Ok(ids.into_iter().flat_map(|x| self.get_by_id(x)).collect())
    }
}
