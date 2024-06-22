use anyhow::*;
use mkwebfont_fontops::font_info::{FontFaceWrapper, FontId};
use roaring::RoaringBitmap;
use std::collections::HashMap;

#[derive(Debug, Default)]
struct SubsetInfo {
    subset: RoaringBitmap,
    range_exclusions: RoaringBitmap,
}

#[derive(Debug, Default)]
pub struct SubsetAssigner {
    assigned_subsets: HashMap<FontId, SubsetInfo>,
}
impl SubsetAssigner {
    fn get_subset(&mut self, id: FontId) -> &mut SubsetInfo {
        self.assigned_subsets.entry(id).or_default()
    }

    pub fn push_stack(&mut self, text: RoaringBitmap, fonts: &[FontFaceWrapper]) -> Result<()> {
        let mut reverse_pass = Vec::new();

        let mut current = text.clone();
        for i in 0..fonts.len() {
            let available_codepoints = fonts[i].all_codepoints();
            let fulfilled_codepoints = available_codepoints & &current;
            self.get_subset(fonts[i].font_id())
                .subset
                .extend(&fulfilled_codepoints);
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
                self.get_subset(fonts[j].font_id())
                    .range_exclusions
                    .extend(&reverse_pass[i]);
            }
        }

        Ok(())
    }
}
