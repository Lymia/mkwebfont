use crate::fonts::FontFaceWrapper;
use enumset::*;
use roaring::RoaringBitmap;
use std::{collections::HashSet, ops::Deref, sync::Arc};

/// A loaded configuration for font splitting.
#[derive(Clone)]
pub struct LoadedSplitterPlan(pub(crate) Arc<SplitterPlanData>);
impl Deref for LoadedSplitterPlan {
    type Target = SplitterPlanData;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl LoadedSplitterPlan {
    pub fn do_split(&self, chars: RoaringBitmap) -> RoaringBitmap {
        if self.subset.is_empty() {
            chars
        } else {
            chars & self.subset.clone()
        }
    }
}

pub struct SplitterPlanData {
    pub preload: RoaringBitmap,
    pub family_config: FontFamilyConfig,
    pub flags: EnumSet<FontFlags>,
    pub subset: RoaringBitmap,
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

#[derive(EnumSetType, Debug)]
pub enum FontFlags {
    PrintReport,
    NoSplitter,
    GfontsSplitter,
    AdjacencySplitter,
}

/// Represents a configuration for font splitting.
#[derive(Clone, Debug)]
pub struct SplitterPlan {
    preload: RoaringBitmap,
    family_config: FontFamilyConfig,
    pub(crate) flags: EnumSet<FontFlags>,
    subset: RoaringBitmap,
}
impl SplitterPlan {
    pub fn new() -> SplitterPlan {
        SplitterPlan {
            preload: Default::default(),
            family_config: FontFamilyConfig::AllFonts,
            flags: Default::default(),
            subset: Default::default(),
        }
    }

    /// A set of characters that should be injected into the same font as the basic latin
    /// characters. This is meant for use with common UI elements used across a website.
    pub fn preload_chars(&mut self, chars: impl Iterator<Item = char>) -> &mut Self {
        for ch in chars {
            self.preload.insert(ch as u32);
        }
        self
    }

    pub fn subset_chars(&mut self, chars: impl Iterator<Item = char>) -> &mut Self {
        for ch in chars {
            self.subset.insert(ch as u32);
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

    /// Prints a report of how much download size the font uses on average
    pub fn print_report(&mut self) -> &mut Self {
        self.flags.insert(FontFlags::PrintReport);
        self
    }

    pub fn no_splitter(&mut self) -> &mut Self {
        self.flags.insert(FontFlags::NoSplitter);
        self
    }

    pub fn gfonts_splitter(&mut self) -> &mut Self {
        self.flags.insert(FontFlags::GfontsSplitter);
        self
    }

    pub fn adjacency_splitter(&mut self) -> &mut Self {
        self.flags.insert(FontFlags::AdjacencySplitter);
        self
    }

    pub fn build(&self) -> LoadedSplitterPlan {
        LoadedSplitterPlan(Arc::new(SplitterPlanData {
            preload: self.preload.clone(),
            family_config: self.family_config.clone(),
            flags: self.flags,
            subset: self.subset.clone(),
        }))
    }
}
