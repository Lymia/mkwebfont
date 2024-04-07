//! Code from <https://github.com/yeslogic/allsorts-tools/blob/master/src/subset.rs>

use allsorts::{
    binary::read::ReadScope,
    font::read_cmap_subtable,
    gsub::{GlyphOrigin, RawGlyph},
    subset,
    tables::{
        cmap::{Cmap, CmapSubtable},
        FontTableProvider,
    },
    tag,
    tinyvec::tiny_vec,
    unicode::VariationSelector,
};
use anyhow::*;
use std::{collections::HashSet, ops::RangeInclusive};
use tracing::debug;

pub fn glyphs_in_font<F: FontTableProvider>(font_provider: &F) -> Result<HashSet<char>> {
    let mut chars = HashSet::new();
    let cmap_data = font_provider.read_table_data(tag::CMAP)?;
    let cmap = ReadScope::new(&cmap_data).read::<Cmap>()?;
    let (_, cmap_subtable) =
        read_cmap_subtable(&cmap)?.ok_or(Error::msg("no suitable cmap sub-table found"))?;
    cmap_subtable.mappings_fn(|ch, _| {
        if let Some(ch) = char::from_u32(ch) {
            chars.insert(ch);
        }
    })?;
    Ok(chars)
}

pub fn glyphs_in_font_subset<F: FontTableProvider>(
    font_provider: &F,
    ranges: &[RangeInclusive<char>],
) -> Result<HashSet<char>> {
    Ok(GlyphInfo::get(font_provider, ranges)?.has_glyph)
}

pub fn subset<F: FontTableProvider>(
    font_provider: &F,
    ranges: &[RangeInclusive<char>],
) -> Result<Vec<u8>> {
    debug!("Creating subset of: {ranges:?}");

    // Work out the glyphs we want to keep from the text
    let mut glyph_ids = HashSet::new();
    for glyph in GlyphInfo::get(font_provider, ranges)?.glyphs {
        glyph_ids.insert(glyph.glyph_index);
    }
    glyph_ids.insert(0);
    let mut glyph_ids: Vec<_> = glyph_ids.into_iter().collect();
    glyph_ids.sort();

    // Subset the font
    let new_font = subset::subset(font_provider, &glyph_ids)?;
    Ok(new_font)
}

struct GlyphInfo {
    has_glyph: HashSet<char>,
    glyphs: Vec<RawGlyph<()>>,
}

impl GlyphInfo {
    fn get<F: FontTableProvider>(
        font_provider: &F,
        ranges: &[RangeInclusive<char>],
    ) -> Result<GlyphInfo> {
        let cmap_data = font_provider.read_table_data(tag::CMAP)?;
        let cmap = ReadScope::new(&cmap_data).read::<Cmap>()?;
        let (_, cmap_subtable) =
            read_cmap_subtable(&cmap)?.ok_or(Error::msg("no suitable cmap sub-table found"))?;

        let mut has_glyph = HashSet::new();
        let mut glyphs = Vec::new();
        for range in ranges {
            for ch in range.clone() {
                if let Some(glyph) = Self::map(&cmap_subtable, ch, None)? {
                    has_glyph.insert(ch);
                    glyphs.push(glyph);
                }
            }
        }
        Ok(GlyphInfo { has_glyph, glyphs })
    }

    fn map(
        cmap_subtable: &CmapSubtable,
        ch: char,
        variation: Option<VariationSelector>,
    ) -> Result<Option<RawGlyph<()>>> {
        if let Result::Ok(Some(glyph_index)) = cmap_subtable.map_glyph(ch as u32) {
            let glyph = Self::make(ch, glyph_index, variation);
            Ok(Some(glyph))
        } else {
            Ok(None)
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
