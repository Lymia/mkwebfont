//! Code from <https://github.com/yeslogic/allsorts-tools/blob/master/src/subset.rs>

use crate::woff2;
use allsorts::{
    binary::read::ReadScope,
    font::read_cmap_subtable,
    font_data::{DynamicFontTableProvider, FontData},
    gsub::{GlyphOrigin, RawGlyph},
    subset,
    tables::{
        cmap::{owned::CmapSubtable, Cmap},
        FontTableProvider,
    },
    tag,
    tinyvec::tiny_vec,
    unicode::VariationSelector,
};
use anyhow::*;
use roaring::RoaringBitmap;

pub struct LoadedFont<'a> {
    font_provider: DynamicFontTableProvider<'a>,
    cmap_subtable: CmapSubtable,
    available_glyphs: RoaringBitmap,
}
impl<'a> LoadedFont<'a> {
    pub fn new(buffer: &'a [u8]) -> Result<LoadedFont<'a>> {
        let font_file = ReadScope::new(buffer).read::<FontData>()?;
        let font_provider = font_file.table_provider(0)?;

        let cmap_data = font_provider.read_table_data(tag::CMAP)?;
        let cmap = ReadScope::new(&cmap_data).read::<Cmap>()?;
        let cmap_subtable = read_cmap_subtable(&cmap)?
            .ok_or(Error::msg("no suitable cmap sub-table found"))?
            .1;

        let mut available_glyphs = RoaringBitmap::new();
        cmap_subtable.mappings_fn(|x, _| {
            available_glyphs.insert(x);
        })?;
        let cmap_subtable = cmap_subtable.to_owned().unwrap();

        Ok(LoadedFont { font_provider, cmap_subtable, available_glyphs })
    }

    pub fn glyphs_in_font(&self, set: &RoaringBitmap) -> RoaringBitmap {
        self.available_glyphs.clone() & set
    }

    pub fn all_glyphs(&self) -> &RoaringBitmap {
        &self.available_glyphs
    }

    pub fn subset(&self, chars: &RoaringBitmap) -> Result<Vec<u8>> {
        // Work out the glyphs we want to keep from the text
        let mut glyph_ids = Vec::new();
        for glyph in self.map_glyphs(chars) {
            glyph_ids.push(glyph.glyph_index);
        }
        glyph_ids.push(0);
        glyph_ids.sort();
        glyph_ids.dedup();

        // Subset the font
        let new_font = subset::subset(&self.font_provider, &glyph_ids)?;
        Ok(woff2::compress(&new_font, "".to_string(), 9, true).unwrap())
    }

    fn map_glyphs(&self, set: &RoaringBitmap) -> Vec<RawGlyph<()>> {
        let mut glyphs = Vec::new();
        for ch in set {
            let ch = char::from_u32(ch).unwrap();
            if let Some(glyph) = Self::map(&self.cmap_subtable, ch, None) {
                glyphs.push(glyph);
            }
        }
        glyphs
    }
    fn map(
        cmap_subtable: &CmapSubtable,
        ch: char,
        variation: Option<VariationSelector>,
    ) -> Option<RawGlyph<()>> {
        if let Result::Ok(Some(glyph_index)) = cmap_subtable.map_glyph(ch as u32) {
            let glyph = Self::make(ch, glyph_index, variation);
            Some(glyph)
        } else {
            None
        }
    }
    fn make(ch: char, glyph_index: u16, variation: Option<VariationSelector>) -> RawGlyph<()> {
        RawGlyph {
            unicodes: tiny_vec![[char; 1] => ch],
            glyph_index,
            liga_component_pos: 0,
            glyph_origin: GlyphOrigin::Char(ch),
            small_caps: false,
            multi_subst_dup: false,
            is_vert_alt: false,
            fake_bold: false,
            fake_italic: false,
            extra_data: (),
            variation,
        }
    }
}
