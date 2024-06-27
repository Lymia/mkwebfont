use crate::plan::subsetter::SubsetDataBuilder;
use anyhow::Result;
use enumset::*;
use mkwebfont_extract_web::WebrootInfo;
use mkwebfont_fontops::font_info::{FontFaceSet, FontFaceWrapper};
use std::{collections::HashSet, ops::Deref, sync::Arc};

mod subsetter;

pub use subsetter::AssignedSubsets;

/// A loaded configuration for font splitting.
#[derive(Clone)]
pub struct LoadedSplitterPlan(pub(crate) Arc<SplitterPlanData>);
pub struct SplitterPlanData {
    pub family_config: FontFamilyConfig,
    pub flags: EnumSet<FontFlags>,
    pub subset_specs: Vec<String>,
}
impl Deref for LoadedSplitterPlan {
    type Target = SplitterPlanData;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl SplitterPlanData {
    pub fn calculate_subsets(
        &self,
        fonts: &FontFaceSet,
        webroot: Option<WebrootInfo>,
    ) -> Result<AssignedSubsets> {
        let mut builder = SubsetDataBuilder::default();
        for spec in &self.subset_specs {
            builder.push_spec(fonts, &spec)?;
        }
        if let Some(webroot) = webroot {
            builder.push_webroot_info(fonts, webroot)?;
        }
        Ok(builder.build())
    }
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
    DoSubsetting,
}

/// Represents a configuration for font splitting.
#[derive(Clone, Debug)]
pub struct SplitterPlan {
    family_config: FontFamilyConfig,
    pub(crate) flags: EnumSet<FontFlags>,
    subset_specs: Vec<String>,
}
impl SplitterPlan {
    pub fn new() -> SplitterPlan {
        SplitterPlan {
            family_config: FontFamilyConfig::AllFonts,
            flags: Default::default(),
            subset_specs: vec![],
        }
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

    /// Enables subsetting.
    pub fn subset(&mut self) -> &mut Self {
        self.flags.insert(FontFlags::DoSubsetting);
        self
    }

    /// Adds a subset spec statement to this plan.
    pub fn subset_spec(&mut self, spec: &str) -> &mut Self {
        self.subset_specs.push(spec.to_string());
        self
    }

    pub fn build(&self) -> LoadedSplitterPlan {
        LoadedSplitterPlan(Arc::new(SplitterPlanData {
            family_config: self.family_config.clone(),
            flags: self.flags,
            subset_specs: self.subset_specs.clone(),
        }))
    }
}
