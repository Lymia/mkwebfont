mod contrib;
mod fonts;
mod splitter;
mod subset_manifest;

pub use fonts::{FontStyle, FontWeight};
pub use splitter::{FontStylesheetEntry, FontStylesheetInfo};

/// A builder for making configuration for splitting webfonts.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct WebfontCtxBuilder {
    store_path: std::path::PathBuf,
    splitter_tuning: Option<String>,
    subset_manifest: Option<String>,
    preload_codepoints: roaring::RoaringBitmap,
    preload_codepoints_in: std::collections::HashMap<String, roaring::RoaringBitmap>,
}
impl WebfontCtxBuilder {
    /// Creates a new builder.
    pub fn new(store_path: &std::path::Path) -> Self {
        WebfontCtxBuilder {
            store_path: store_path.to_path_buf(),
            splitter_tuning: None,
            subset_manifest: None,
            preload_codepoints: Default::default(),
            preload_codepoints_in: Default::default(),
        }
    }

    /// Adds a splitter tuning file.
    pub fn add_splitter_tuning(&mut self, path: &str) {
        self.splitter_tuning = Some(path.to_string());
    }

    /// Adds a subset manifest file.
    pub fn add_subset_manifest(&mut self, path: &str) {
        self.subset_manifest = Some(path.to_string());
    }

    /// Preload certain characters into every font loaded in this context.
    pub fn preload(&mut self, chars: impl Iterator<Item = char>) {
        self.preload_codepoints.extend(chars.map(|x| x as u32));
    }

    /// Preload certain characters into a given font family.
    pub fn preload_in(&mut self, font: &str, chars: impl Iterator<Item = char>) {
        self.preload_codepoints_in
            .entry(font.to_string())
            .or_default()
            .extend(chars.map(|x| x as u32));
    }

    /// Builds the context, and checks its arguments properly.
    pub fn build(self) -> anyhow::Result<WebfontCtx> {
        Ok(WebfontCtx {
            store_path: self.store_path,
            preload_codepoints: self.preload_codepoints,
            preload_codepoints_in: self.preload_codepoints_in,
            tuning: match self.splitter_tuning {
                None => toml::from_str(include_str!("splitter_default_tuning.toml"))?,
                Some(data) => toml::from_str(&data)?,
            },
            data: match self.subset_manifest {
                None => subset_manifest::WebfontData::load(include_str!(
                    "subset_manifest_default.toml"
                ))?,
                Some(data) => subset_manifest::WebfontData::load(&data)?,
            },
        })
    }
}

/// A particular configuration for splitting webfonts.
#[derive(Clone, Debug)]
pub struct WebfontCtx {
    pub(crate) store_path: std::path::PathBuf,
    pub(crate) preload_codepoints: roaring::RoaringBitmap,
    pub(crate) preload_codepoints_in: std::collections::HashMap<String, roaring::RoaringBitmap>,
    pub(crate) tuning: splitter::TuningParameters,
    pub(crate) data: subset_manifest::WebfontData,
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
    split_ctx: &WebfontCtx,
    fonts: &[LoadedFont],
) -> anyhow::Result<Vec<FontStylesheetInfo>> {
    let mut out = Vec::new();
    for font in fonts {
        out.push(splitter::split_webfont(split_ctx, &font.underlying)?);
    }
    Ok(out)
}
