use anyhow::*;
use mkwebfont_extract_web::WebrootInfo;
use mkwebfont_fontops::font_info::{FontFaceSet, FontFaceWrapper, FontId};
use roaring::RoaringBitmap;
use std::{collections::HashMap, sync::LazyLock};

#[derive(Clone, Debug, Default)]
struct SubsetInfo {
    subset: RoaringBitmap,
    exclusion: RoaringBitmap,
    preload: RoaringBitmap,
    range_exclusions: RoaringBitmap,
}

#[derive(Clone, Debug, Default)]
pub struct AssignedSubsets {
    disabled: bool,
    assigned_subsets: HashMap<FontId, SubsetInfo>,
    all_subset: RoaringBitmap,
    all_exclusion: RoaringBitmap,
    all_preload: RoaringBitmap,
    fallback_required: RoaringBitmap,
}
impl AssignedSubsets {
    pub fn disabled() -> &'static AssignedSubsets {
        static ASSIGNED: LazyLock<AssignedSubsets> = LazyLock::new(|| {
            let mut assigned = AssignedSubsets::default();
            assigned.disabled = true;
            assigned
        });
        &ASSIGNED
    }

    fn get_subset(&self, id: FontId) -> &SubsetInfo {
        static EMPTY: LazyLock<SubsetInfo> = LazyLock::new(|| SubsetInfo::default());
        self.assigned_subsets.get(&id).unwrap_or_else(|| &EMPTY)
    }

    pub fn get_used_chars(&self, font: &FontFaceWrapper) -> RoaringBitmap {
        if self.disabled {
            font.all_codepoints().clone()
        } else {
            let info = self.get_subset(font.font_id());
            let subsets = &info.subset | &self.all_subset;
            let excludes = &info.exclusion | &self.all_exclusion;
            (subsets - excludes) & font.all_codepoints()
        }
    }

    pub fn get_preload_chars(&self, font: &FontFaceWrapper) -> RoaringBitmap {
        if self.disabled {
            RoaringBitmap::new()
        } else {
            let info = self.get_subset(font.font_id());
            self.get_used_chars(font) & (&info.preload | &self.all_preload)
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SubsetDataBuilder {
    subsets: AssignedSubsets,
}
impl SubsetDataBuilder {
    fn get_subset_mut(&mut self, id: FontId) -> &mut SubsetInfo {
        self.subsets.assigned_subsets.entry(id).or_default()
    }

    fn push_stack(&mut self, text: RoaringBitmap, fonts: &[Vec<FontFaceWrapper>]) -> Result<()> {
        let mut reverse_pass = Vec::new();

        let mut current = text.clone();
        for i in 0..fonts.len() {
            ensure!(fonts[i].len() > 0, "Fonts lists cannot be empty!");
            let available_codepoints = fonts[i][0].all_codepoints();
            for font in &fonts[i][1..] {
                ensure!(
                    font.all_codepoints() == available_codepoints,
                    "Fonts lists must have the same character sets."
                );
            }
            let fulfilled_codepoints = available_codepoints & &current;

            for j in 0..fonts[i].len() {
                self.get_subset_mut(fonts[i][j].font_id())
                    .subset
                    .extend(&fulfilled_codepoints);
            }
            reverse_pass.push(fulfilled_codepoints.clone());
            current = current - fulfilled_codepoints;
        }

        if !current.is_empty() {
            self.subsets.fallback_required.extend(current);
        }

        for i in 0..fonts.len() {
            for j in 0..i {
                for k in 0..fonts[i].len() {
                    self.get_subset_mut(fonts[j][k].font_id())
                        .range_exclusions
                        .extend(&reverse_pass[i]);
                }
            }
        }

        Ok(())
    }

    fn push_exclusion(&mut self, text: RoaringBitmap, fonts: &[FontFaceWrapper]) {
        for font in fonts {
            self.get_subset_mut(font.font_id()).exclusion.extend(&text);
        }
    }

    fn push_preload(&mut self, text: RoaringBitmap, fonts: &[FontFaceWrapper]) {
        for font in fonts {
            self.get_subset_mut(font.font_id()).preload.extend(&text);
        }
    }

    fn load_fonts(fonts: &FontFaceSet, spec: &str) -> Result<Vec<FontFaceWrapper>> {
        let mut list = Vec::new();
        for font in spec.split(',') {
            list.push(fonts.resolve(font.trim())?.clone());
        }
        Ok(list)
    }

    fn load_fonts_list(fonts: &FontFaceSet, spec: &str) -> Result<Vec<Vec<FontFaceWrapper>>> {
        let mut list = Vec::new();
        for font in Self::load_fonts(fonts, spec)? {
            list.push(vec![font]);
        }
        Ok(list)
    }

    fn load_charset(spec: &str) -> Result<RoaringBitmap> {
        fn chars_to_bitmap(chars: &str) -> RoaringBitmap {
            let mut roaring = RoaringBitmap::new();
            for ch in chars.chars() {
                roaring.insert(ch as u32);
            }
            roaring
        }

        if spec.starts_with("@") {
            Ok(chars_to_bitmap(&std::fs::read_to_string(&spec[1..])?))
        } else if spec.starts_with("#") {
            let mut roaring = RoaringBitmap::new();
            for section in spec[1..].split(',') {
                let section = section.trim();
                let (start, end) = if section.starts_with("U+") {
                    let section = &section[2..];
                    if section.contains("-") {
                        let mut iter = section.split('-');
                        let start = u32::from_str_radix(iter.next().unwrap(), 16)?;
                        let end = u32::from_str_radix(iter.next().unwrap(), 16)?;
                        ensure!(iter.next().is_none(), "Multiple `-` in unicode-range spec.");
                        (start, end)
                    } else if section.contains("?") {
                        let start = u32::from_str_radix(&section.replace('?', "0"), 16)?;
                        let end = u32::from_str_radix(&section.replace('?', "F"), 16)?;
                        (start, end)
                    } else {
                        let val = u32::from_str_radix(section, 16)?;
                        (val, val)
                    }
                } else {
                    panic!("unicode-range spec does not start with `U+`?");
                };
                for ch in start..=end {
                    roaring.insert(ch);
                }
            }
            Ok(roaring)
        } else {
            Ok(chars_to_bitmap(spec))
        }
    }

    pub fn push_spec(&mut self, fonts: &FontFaceSet, spec: &str) -> Result<()> {
        if spec.starts_with("@") {
            let contents = std::fs::read_to_string(&spec[1..])?;
            for line in contents.split('\n') {
                self.push_spec(fonts, line)?;
            }
        } else if spec.starts_with("exclude:") {
            let spec = &spec["exclude:".len()..];
            if spec.starts_with("*:") {
                self.subsets
                    .all_exclusion
                    .extend(Self::load_charset(&spec[2..])?);
            } else {
                self.push_exclusion(Self::load_charset(spec)?, &Self::load_fonts(fonts, spec)?);
            }
        } else if spec.starts_with("preload:") {
            let spec = &spec["preload:".len()..];
            if spec.starts_with("*:") {
                self.subsets
                    .all_preload
                    .extend(Self::load_charset(&spec[2..])?);
            } else {
                self.push_preload(Self::load_charset(spec)?, &Self::load_fonts(fonts, spec)?);
            }
        } else {
            if spec.starts_with("*:") {
                self.subsets
                    .all_subset
                    .extend(Self::load_charset(&spec[2..])?);
            } else {
                self.push_stack(Self::load_charset(spec)?, &Self::load_fonts_list(fonts, spec)?)?;
            }
        }
        Ok(())
    }

    /// This function expects that all fonts present in the `TextInfo` are loaded!!
    /// That is not the job of this function.
    pub fn push_webroot_info(&mut self, fonts: &FontFaceSet, text: WebrootInfo) -> Result<()> {
        for stack in text.font_stacks {
            for sample in stack.samples {
                let mut list = Vec::new();
                for font in &*stack.stack {
                    list.push(fonts.resolve_by_styles(
                        &font,
                        sample.used_styles,
                        &sample.used_weights,
                    ))
                }
            }
        }
        todo!()
    }

    pub fn build(self) -> AssignedSubsets {
        self.subsets
    }
}
