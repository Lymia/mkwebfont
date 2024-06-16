use crate::fonts::FontFaceWrapper;
use anyhow::*;
use mkwebfont_common::hashing::WyHashBuilder;
use roaring::RoaringBitmap;
use std::{
    collections::HashMap,
    sync::atomic::{AtomicUsize, Ordering},
};

#[derive(Debug, Hash, Eq, PartialEq)]
struct FontInfo(String, String, String);
impl FontInfo {
    fn for_font(font: &FontFaceWrapper) -> Self {
        FontInfo(
            font.font_family().to_string(),
            font.font_style().to_string(),
            font.font_version().to_string(),
        )
    }
}

#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
struct FontId(usize);
impl FontId {
    fn new() -> Self {
        const COUNTER: AtomicUsize = AtomicUsize::new(0);
        FontId(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

#[derive(Debug, Default)]
struct FontIdAssigner(HashMap<FontInfo, FontId, WyHashBuilder>);
impl FontIdAssigner {
    fn get_id(&mut self, font: &FontFaceWrapper) -> FontId {
        *self
            .0
            .entry(FontInfo::for_font(font))
            .or_insert_with(FontId::new)
    }
}

#[derive(Debug, Default)]
struct SubsetInfo {
    subset: RoaringBitmap,
    range_exclusions: RoaringBitmap,
}

#[derive(Debug, Default)]
pub struct SubsetAssigner {
    ids: FontIdAssigner,
    assigned_subsets: HashMap<FontId, SubsetInfo>,
}
impl SubsetAssigner {
    fn get_subset(&mut self, id: FontId) -> &mut SubsetInfo {
        self.assigned_subsets.entry(id).or_default()
    }

    pub fn push_stack(&mut self, text: RoaringBitmap, fonts: &[FontFaceWrapper]) -> Result<()> {
        let ids: Vec<_> = fonts.iter().map(|x| self.ids.get_id(x)).collect();
        let mut reverse_pass = Vec::new();

        let mut current = text.clone();
        for i in 0..fonts.len() {
            let available_codepoints = fonts[i].all_codepoints();
            let fulfilled_codepoints = available_codepoints & &current;
            self.get_subset(ids[i]).subset.extend(&fulfilled_codepoints);
            reverse_pass.push(fulfilled_codepoints.clone());
            current = current - fulfilled_codepoints;
        }

        if !current.is_empty() {
            let mut stack = String::new();
            let mut is_first = true;
            for font in fonts {
                if !is_first {
                    stack.push_str(", ");
                }
                is_first = false;
                stack.push_str(font.font_family())
            }
            bail!("{} codepoints are not found in the font stack: {stack}", current.len());
        }

        for i in 0..fonts.len() {
            for j in 0..i {
                self.get_subset(ids[j])
                    .range_exclusions
                    .extend(&reverse_pass[i]);
            }
        }

        Ok(())
    }
}
