use anyhow::*;
use arcstr::ArcStr;
use mkwebfont_common::{character_set::CharacterSet, hashing::WyHashMap};
use mkwebfont_extract_web::WebrootInfo;
use mkwebfont_fontops::font_info::{FontFaceSet, FontFaceWrapper, FontId};
use std::{
    fmt::Debug,
    sync::{Arc, LazyLock},
};

#[derive(Clone, Debug, Default)]
struct SubsetInfo {
    subset: CharacterSet,
    exclusion: CharacterSet,
    preload: CharacterSet,
    range_exclusions: CharacterSet,
}

#[derive(Clone, Debug, Default)]
pub struct AssignedSubsets {
    disabled: bool,
    assigned_subsets: WyHashMap<FontId, SubsetInfo>,
    all_subset: CharacterSet,
    all_exclusion: CharacterSet,
    all_preload: CharacterSet,

    fallback_required: CharacterSet,
    fallback_info: WyHashMap<Arc<[ArcStr]>, CharacterSet>,
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

    pub fn get_used_chars(&self, font: &FontFaceWrapper) -> CharacterSet {
        if self.disabled {
            font.all_codepoints().clone()
        } else {
            let info = self.get_subset(font.font_id());
            let subsets = &info.subset | &self.all_subset;
            let excludes = &info.exclusion | &self.all_exclusion;
            (subsets - excludes) & font.all_codepoints()
        }
    }

    pub fn get_range_exclusion(&self, font: &FontFaceWrapper) -> CharacterSet {
        if self.disabled {
            font.all_codepoints().clone()
        } else {
            let info = self.get_subset(font.font_id());
            self.get_used_chars(font) | &info.range_exclusions
        }
    }

    pub fn get_preload_chars(&self, font: &FontFaceWrapper) -> CharacterSet {
        if self.disabled {
            CharacterSet::new()
        } else {
            let info = self.get_subset(font.font_id());
            self.get_used_chars(font) & (&info.preload | &self.all_preload)
        }
    }

    pub fn get_fallback_chars(&self) -> &CharacterSet {
        &self.fallback_required
    }

    pub fn get_fallback_info(&self) -> &WyHashMap<Arc<[ArcStr]>, CharacterSet> {
        &self.fallback_info
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

    pub fn push_stack(
        &mut self,
        text: CharacterSet,
        fonts: &[impl AsRef<[FontFaceWrapper]>],
    ) -> Result<()> {
        let mut reverse_pass = Vec::new();

        let mut current = text.clone();
        for i in 0..fonts.len() {
            let font = fonts[i].as_ref();

            ensure!(font.len() > 0, "Fonts lists cannot be empty!");
            let mut fulfilled_codepoints = font[0].all_codepoints().clone();
            for font in &font[1..] {
                fulfilled_codepoints &= font.all_codepoints();
            }
            fulfilled_codepoints &= &current;
            let fulfilled_codepoints = fulfilled_codepoints;

            for j in 0..font.len() {
                self.get_subset_mut(font[j].font_id())
                    .subset
                    .extend(&fulfilled_codepoints);
            }
            reverse_pass.push(fulfilled_codepoints.clone());
            current = current - fulfilled_codepoints;
        }

        if !current.is_empty() {
            self.subsets.fallback_required.extend(&current);
        }

        for i in 0..fonts.len() {
            for j in 0..i {
                for k in 0..fonts[i].as_ref().len() {
                    self.get_subset_mut(fonts[j].as_ref()[k].font_id())
                        .range_exclusions
                        .extend(&reverse_pass[i]);
                }
            }
        }

        for font_stack in fonts {
            let mut new_stack = Vec::new();
            for font in font_stack.as_ref() {
                new_stack.push(ArcStr::from(font.font_family().to_lowercase()));
            }
            *self
                .subsets
                .fallback_info
                .entry(new_stack.into())
                .or_default() |= &current;
        }

        Ok(())
    }

    fn push_exclusion(&mut self, text: CharacterSet, fonts: &[FontFaceWrapper]) {
        for font in fonts {
            self.get_subset_mut(font.font_id()).exclusion.extend(&text);
        }
    }

    fn push_preload(&mut self, text: CharacterSet, fonts: &[FontFaceWrapper]) {
        for font in fonts {
            self.get_subset_mut(font.font_id()).preload.extend(&text);
        }
    }

    fn load_fonts(fonts: &FontFaceSet, spec: &str) -> Result<Vec<FontFaceWrapper>> {
        let mut list = Vec::new();
        for font_name in spec.split(',') {
            for font in fonts.resolve_all(font_name.trim())? {
                list.push(font.clone());
            }
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

    fn load_charset(spec: &str) -> Result<CharacterSet> {
        fn chars_to_bitmap(chars: &str) -> CharacterSet {
            let mut roaring = CharacterSet::new();
            for ch in chars.chars() {
                roaring.insert(ch as u32);
            }
            roaring
        }

        if spec.starts_with("@") {
            Ok(chars_to_bitmap(&std::fs::read_to_string(&spec[1..])?))
        } else if spec.starts_with("#") {
            let mut roaring = CharacterSet::new();
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

    fn split_two(spec: &str) -> Result<(&str, &str)> {
        if !spec.contains(':') {
            bail!("Incorrect subset data format.");
        } else {
            let mut split = spec.splitn(2, ':');
            let fst = split.next().unwrap();
            let snd = split.next().unwrap();
            Ok((fst, snd))
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
                let (fst, snd) = Self::split_two(spec)?;
                self.push_exclusion(Self::load_charset(snd)?, &Self::load_fonts(fonts, fst)?);
            }
        } else if spec.starts_with("preload:") {
            let spec = &spec["preload:".len()..];
            if spec.starts_with("*:") {
                self.subsets
                    .all_preload
                    .extend(Self::load_charset(&spec[2..])?);
            } else {
                let (fst, snd) = Self::split_two(spec)?;
                self.push_preload(Self::load_charset(snd)?, &Self::load_fonts(fonts, fst)?);
            }
        } else {
            if spec.starts_with("*:") {
                self.subsets
                    .all_subset
                    .extend(Self::load_charset(&spec[2..])?);
            } else {
                let (fst, snd) = Self::split_two(spec)?;
                self.push_stack(Self::load_charset(snd)?, &Self::load_fonts_list(fonts, fst)?)?;
            }
        }
        Ok(())
    }

    /// This function expects that all fonts present in the `TextInfo` are loaded!!
    /// That is not the job of this function.
    pub fn push_webroot_info(&mut self, fonts: &FontFaceSet, text: &WebrootInfo) -> Result<()> {
        for stack in &text.font_stacks {
            for sample in &stack.samples {
                let mut list: Vec<Vec<_>> = Vec::new();
                for font in &*stack.stack {
                    list.push(
                        fonts
                            .resolve_by_styles(&font, sample.used_styles, &sample.used_weights)?
                            .into_iter()
                            .map(|x| x.clone())
                            .collect(),
                    )
                }

                let mut chars = CharacterSet::new();
                for ch in sample.glyphs().chars() {
                    chars.insert(ch as u32);
                }
                self.push_stack(chars, &list)?;
            }
        }
        Ok(())
    }

    pub fn build(self) -> AssignedSubsets {
        self.subsets
    }
}
