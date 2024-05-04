use crate::fonts::FontFaceWrapper;
use roaring::RoaringBitmap;
use std::{collections::HashSet, ops::Deref, sync::Arc};

/// A loaded configuration for subsetting.
#[derive(Clone)]
pub struct LoadedSubsetPlan(pub(crate) Arc<SubsetPlanData>);
impl Deref for LoadedSubsetPlan {
    type Target = SubsetPlanData;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
pub struct SubsetPlanData {
    pub preload: RoaringBitmap,
    pub family_config: FontFamilyConfig,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FontFamilyConfig {
    AllFonts,
    Whitelist(HashSet<String>),
    Blacklist(HashSet<String>),
}
impl FontFamilyConfig {
    pub fn check_font(&self, font_face: &FontFaceWrapper) -> bool {
        match self {
            FontFamilyConfig::AllFonts => true,
            FontFamilyConfig::Whitelist(list) => list.contains(font_face.font_family()),
            FontFamilyConfig::Blacklist(list) => !list.contains(font_face.font_family()),
        }
    }
}

/// Represents a configuration for subsetting.
#[derive(Clone, Debug)]
pub struct SubsetPlan {
    preload: RoaringBitmap,
    family_config: FontFamilyConfig,
}
impl SubsetPlan {
    pub fn new() -> SubsetPlan {
        SubsetPlan { preload: Default::default(), family_config: FontFamilyConfig::AllFonts }
    }

    /// A set of characters that should be injected into the same font as the basic latin
    /// characters. This is meant for use with common UI elements used across a website.
    pub fn preload_chars(&mut self, chars: impl Iterator<Item = char>) -> &mut Self {
        for ch in chars {
            self.preload.insert(ch as u32);
        }
        self
    }

    /// Sets a list of font families to whitelist. Font families not in the list will not be
    /// processed.
    ///
    /// This is useful when working with large font collections.
    pub fn whitelist_fonts(
        &mut self,
        fonts: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> &mut Self {
        assert_eq!(
            self.family_config,
            FontFamilyConfig::AllFonts,
            "`whitelist_fonts` and `exclude_fonts` may only be called once.",
        );
        self.family_config = FontFamilyConfig::Whitelist(
            fonts.into_iter().map(|x| x.as_ref().to_string()).collect(),
        );
        self
    }

    /// Sets a list of font families to blacklist. Font families in the list will not be processed.
    ///
    /// This is useful when working with large font collections.
    pub fn blacklist_fonts(
        &mut self,
        fonts: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> &mut Self {
        assert_eq!(
            self.family_config,
            FontFamilyConfig::AllFonts,
            "`whitelist_fonts` and `exclude_fonts` may only be called once.",
        );
        self.family_config = FontFamilyConfig::Blacklist(
            fonts.into_iter().map(|x| x.as_ref().to_string()).collect(),
        );
        self
    }

    pub fn build(&self) -> LoadedSubsetPlan {
        LoadedSubsetPlan(Arc::new(SubsetPlanData {
            preload: self.preload.clone(),
            family_config: self.family_config.clone(),
        }))
    }
}
