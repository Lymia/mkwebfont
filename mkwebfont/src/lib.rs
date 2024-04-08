mod contrib;
mod fonts;
mod splitter;
mod subset_manifest;

pub use fonts::{FontStyle, FontWeight};
pub use splitter::{FontStylesheetEntry, FontStylesheetInfo};

/// A particular configuration for splitting webfonts.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct SplitWebfontCtx {
    pub splitter_tuning: Option<String>,
    pub preload_codepoints: roaring::RoaringBitmap,
}
impl Default for SplitWebfontCtx {
    fn default() -> Self {
        SplitWebfontCtx { splitter_tuning: None, preload_codepoints: Default::default() }
    }
}

#[derive(Debug)]
pub struct ActiveSplitWebfontCtx<'a> {
    pub(crate) ctx: &'a mut SplitWebfontCtx,
}

/// A loaded font.
///
/// This may be used to filter font collections or simply subset multiple fonts in one operation.
pub struct LoadedFont<'a> {
    underlying: fonts::LoadedFont<'a>,
}
impl<'a> LoadedFont<'a> {
    /// Loads all fonts present in a given binary font data.
    pub fn load(font_data: &'a [u8]) -> anyhow::Result<Vec<Self>> {
        Ok(fonts::LoadedFont::load(font_data)?
            .into_iter()
            .map(|x| LoadedFont { underlying: x })
            .collect())
    }

    /// Returns the name of the font family
    pub fn font_family(&self) -> &str {
        &self.underlying.font_name
    }

    /// Returns the font's style
    pub fn font_style(&self) -> &str {
        &self.underlying.font_style
    }

    /// Returns the font version
    pub fn font_version(&self) -> &str {
        &self.underlying.font_version
    }

    /// Returns whether the font is a variable font
    pub fn is_variable(&self) -> bool {
        self.underlying.is_variable
    }
}

pub fn split_webfont(
    split_ctx: &SplitWebfontCtx,
    font_path: &std::path::Path,
    store_path: &std::path::Path,
) -> anyhow::Result<Vec<FontStylesheetInfo>> {
    splitter::split_webfont(
        split_ctx,
        None,
        subset_manifest::WebfontDataCtx::load(),
        &std::fs::read(font_path)?,
        store_path,
    )
}
