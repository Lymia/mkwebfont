use crate::{
    plan::{AssignedSubsets, FontFlags},
    splitter,
};
use anyhow::{bail, Result};
use arcstr::ArcStr;
use mkwebfont_common::{
    character_set::CharacterSet,
    download_cache::DownloadInfo,
    hashing::{WyHashMap, WyHashSet},
    join_set::JoinSet,
};
use mkwebfont_extract_web::{RewriteContext, WebrootInfo, WebrootInfoExtractor};
use mkwebfont_fontops::{
    font_info::{FontFaceSet, FontFaceWrapper},
    gfonts::gfonts_list::GfontsList,
};
use std::{
    fmt::Debug,
    path::{Path, PathBuf},
    sync::Arc,
};
use tracing::{info, info_span, Instrument};

pub use crate::plan::SplitterPlan;
use crate::splitter::FALLBACK_FONT_NAME;
pub use mkwebfont_fontops::{
    font_info::{FontStyle, FontWeight},
    subsetter::{SubsetInfo, WebfontInfo},
};

/// A loaded font.
///
/// This may be used to filter font collections or simply subset multiple fonts in one operation.
#[derive(Clone, Debug)]
pub struct LoadedFont {
    underlying: FontFaceWrapper,
}
impl LoadedFont {
    /// Loads all fonts present in a given binary font data.
    pub fn load(font_data: &[u8]) -> Result<Vec<Self>> {
        Ok(FontFaceWrapper::load(None, font_data)?
            .into_iter()
            .map(|x| LoadedFont { underlying: x })
            .collect())
    }

    /// Loads all fonts present in a given file.
    pub fn load_path(path: &Path) -> Result<Vec<Self>> {
        Ok(FontFaceWrapper::load(
            path.file_name().map(|x| x.to_string_lossy().to_string()),
            std::fs::read(path)?,
        )?
        .into_iter()
        .map(|x| LoadedFont { underlying: x })
        .collect())
    }

    /// Returns the list of codepoints in the loaded font.
    pub fn codepoints(&self) -> CharacterSet {
        self.underlying.all_codepoints().clone()
    }

    /// Returns the name of the font family
    pub fn font_family(&self) -> &str {
        self.underlying.font_family()
    }

    /// Returns the font's style
    pub fn font_style(&self) -> &str {
        self.underlying.font_style()
    }

    /// Returns the font version
    pub fn font_version(&self) -> &str {
        self.underlying.font_version()
    }

    /// Returns whether the font is a variable font
    pub fn is_variable(&self) -> bool {
        self.underlying.is_variable()
    }
}

/// The builder for a set of loaded fonts.
#[derive(Default)]
pub struct LoadedFontSetBuilder {
    fonts: Vec<LoadedFont>,
    paths: Vec<PathBuf>,
    gfonts: Vec<String>,
    webroot: Option<Webroot>,
}
impl LoadedFontSetBuilder {
    /// Creates a new empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads fonts from disk.
    pub fn load_from_disk(mut self, paths: impl IntoIterator<Item = impl AsRef<Path>>) -> Self {
        self.paths
            .extend(paths.into_iter().map(|x| x.as_ref().to_path_buf()));
        self
    }

    /// Loads fonts from the Google Fonts repository.
    ///
    /// This does *NOT* use the Google Fonts service, but rather the repository on Github!
    pub fn load_from_gfonts(mut self, fonts: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.gfonts
            .extend(fonts.into_iter().map(|x| x.as_ref().to_string()));
        self
    }

    /// Loads the fonts required for a given webroot.
    pub fn add_from_webroot(mut self, webroot: &Webroot) -> Self {
        assert!(self.webroot.is_none());
        self.webroot = Some(webroot.clone());
        self
    }

    /// Adds a font to the font set buidler.
    pub fn add_font(mut self, font: LoadedFont) -> Self {
        self.fonts.push(font);
        self
    }

    /// Adds a set of fonts to the font set builder.
    pub fn add_fonts(mut self, fonts: &[LoadedFont]) -> Self {
        for font in fonts {
            self.fonts.push(font.clone());
        }
        self
    }

    /// Loads a font from a byte array.
    pub fn load(self, font_data: &[u8]) -> Result<Self> {
        Ok(self.add_fonts(&LoadedFont::load(font_data)?))
    }

    /// Loads a font from a file.
    pub fn load_path(self, path: &Path) -> Result<Self> {
        Ok(self.add_fonts(&LoadedFont::load_path(path)?))
    }

    /// Merges two font set builders.
    pub fn merge(mut self, other: LoadedFontSetBuilder) {
        self.fonts.extend(other.fonts);
        self.paths.extend(other.paths);
        self.gfonts.extend(other.gfonts);
    }

    /// Builds the final font set.
    pub async fn build(self) -> Result<LoadedFontSet> {
        let mut joins = JoinSet::new();
        if !self.paths.is_empty() {
            let paths = self.paths;
            joins.spawn(load_fonts_from_disk(paths));
        }
        if !self.gfonts.is_empty() {
            let gfonts = self.gfonts;
            joins.spawn(load_fonts_from_gfonts(gfonts));
        }

        let mut fonts = Vec::new();
        fonts.extend(joins.join_vec().await?);
        fonts.extend(self.fonts);

        if let Some(webroot) = self.webroot {
            info!("Resolving remaining webroot fonts...");
            let font_set = FontFaceSet::build(fonts.iter().map(|x| x.underlying.clone()));
            fonts.extend(load_fonts_from_webroot(webroot, font_set).await?);
        }

        let font_set = FontFaceSet::build(fonts.into_iter().map(|x| x.underlying));
        info!("{} total fonts loaded!", font_set.as_list().len());
        Ok(LoadedFontSet { font_set })
    }
}

/// A set of loaded fonts.
///
/// Create these with [`LoadedFontSetBuilder`].
pub struct LoadedFontSet {
    font_set: FontFaceSet,
}
impl LoadedFontSet {
    /// Retrieves a font by name.
    pub fn resolve(&self, name: &str) -> Result<Vec<LoadedFont>> {
        Ok(self
            .font_set
            .resolve_all(name)?
            .into_iter()
            .map(|x| LoadedFont { underlying: x.clone() })
            .collect())
    }
}

/// A fast function for loading remaining fonts in a webroot from Google Fonts
async fn load_fonts_from_webroot(
    webroot: Webroot,
    existing: FontFaceSet,
) -> Result<Vec<LoadedFont>> {
    fn check_font(
        existing: &FontFaceSet,
        name: &str,
        style: FontStyle,
        weight: FontWeight,
    ) -> Result<Option<&'static DownloadInfo>> {
        if existing.resolve_by_style(name, style, weight).is_ok() {
            Ok(None)
        } else {
            if let Some(font) = GfontsList::find_font(name) {
                if let Some(style) = font.find_nearest_match(style, weight) {
                    Ok(Some(&style.info))
                } else {
                    bail!("No such font exists on Google Fonts: {name} / {style} / {weight}");
                }
            } else {
                bail!("No such font exists on Google Fonts: {name}");
            }
        }
    }

    let mut infos = WyHashSet::default();
    for stacks in &webroot.0.font_stacks {
        for font in &*stacks.stack {
            for sample in &stacks.samples {
                for style in sample.used_styles {
                    for weight in &*sample.used_weights {
                        if let Some(info) = check_font(&existing, font.as_str(), style, *weight)? {
                            if infos.insert(info) {
                                info!("Loading font: (Google Fonts) {font} / {style} / {weight}");
                            }
                        }
                    }
                }
            }
        }
    }

    let mut joins = JoinSet::new();
    for info in infos {
        joins.spawn(async move {
            let data = info.load().await?;
            LoadedFont::load(&data)
        });
    }

    let fonts = joins.join_vec().await?;
    info!("Loaded {} required font files from Google Fonts...", fonts.len());
    Ok(fonts)
}

/// A fast function for loading fonts from Google Fonts.
async fn load_fonts_from_gfonts(
    names: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<Vec<LoadedFont>> {
    let info = GfontsList::load();
    let short_rev = &info.repo_revision[..7];
    info!("Using Google Fonts repository from {} (r{short_rev})", info.repo_short_date);

    let mut joins = JoinSet::new();
    for name in names {
        let name = name.as_ref();
        let font_info = GfontsList::find_font(name);
        if let Some(info) = font_info {
            for style in &info.styles {
                let name = name.to_string();
                joins.spawn(async move {
                    info!("Loading font: (Google Fonts) {name} / {style}");
                    let data = style.info.load().await?;
                    LoadedFont::load(&data)
                })
            }
        } else {
            bail!("No such font exists on Google Fonts: {name}");
        }
    }

    let fonts = joins.join_vec().await?;
    info!("Loaded {} font files from Google Fonts...", fonts.len());
    Ok(fonts)
}

/// A fast function for loading fonts from disk.
async fn load_fonts_from_disk(
    paths: impl IntoIterator<Item = impl AsRef<Path>>,
) -> Result<Vec<LoadedFont>> {
    let mut joins = JoinSet::new();
    for path in paths {
        let path = path.as_ref().to_path_buf();
        joins.spawn(async move {
            info!("Loading font: (File) {}", path.display());
            LoadedFont::load_path(&path)
        });
    }

    let fonts = joins.join_vec().await?;
    info!("Loaded {} font files from disk...", fonts.len());
    Ok(fonts)
}

#[derive(Debug, Clone)]
pub struct Webroot(Arc<WebrootInfo>);
impl Webroot {
    pub async fn load(path: &Path) -> Result<Webroot> {
        let extractor = WebrootInfoExtractor::new();
        extractor.push_webroot(path, &[]).await?;
        Ok(Webroot(Arc::new(extractor.build().await)))
    }

    pub async fn rewrite_webroot(&self, ctx: RewriteContext) -> Result<()> {
        self.0.rewrite_webroot(ctx).await
    }
}

#[derive(Clone, Debug)]
pub struct WebfontResults {
    pub webfonts: Vec<Arc<WebfontInfo>>,
    fallback_info: WyHashMap<Arc<[ArcStr]>, CharacterSet>,
    webroot: Option<Webroot>,
}
impl WebfontResults {
    fn rewrite_ctx(&self, store_path: PathBuf, store_uri: Option<String>) -> RewriteContext {
        RewriteContext {
            fallback_font_name: FALLBACK_FONT_NAME.to_string(),
            fallback_info: self.fallback_info.clone(),
            webfonts: self.webfonts.clone(),
            store_path,
            store_uri,
        }
    }

    pub fn has_webroot(&self) -> bool {
        self.webroot.is_some()
    }

    pub async fn rewrite_webroot(
        &self,
        store_path: impl AsRef<Path>,
        store_uri: Option<impl AsRef<str>>,
    ) -> Result<()> {
        let rewrite_ctx = self.rewrite_ctx(
            store_path.as_ref().to_path_buf(),
            store_uri.map(|x| x.as_ref().to_string()),
        );
        if let Some(webroot) = &self.webroot {
            webroot.rewrite_webroot(rewrite_ctx).await
        } else {
            bail!("No webroot is available.");
        }
    }

    pub fn produce_css(
        &self,
        store_path: impl AsRef<Path>,
        store_uri: Option<impl AsRef<str>>,
    ) -> Result<String> {
        if store_uri.is_none() {
            bail!("Cannot generate detached .css files without an explicit store URI.")
        }
        let rewrite_ctx = self.rewrite_ctx(
            store_path.as_ref().to_path_buf(),
            Some(store_uri.unwrap().as_ref().to_string()),
        );
        rewrite_ctx.generate_font_css()
    }

    pub fn write_webfonts(&self, store_path: impl AsRef<Path>) -> Result<()> {
        for font in &self.webfonts {
            font.write_to_store(store_path.as_ref())?;
        }
        Ok(())
    }
}

pub async fn process_webfont(
    plan: &SplitterPlan,
    fonts: &LoadedFontSet,
    webroot: Option<&Webroot>,
) -> Result<WebfontResults> {
    let plan = plan.build();

    let assigned = Arc::new(if plan.flags.contains(FontFlags::DoSubsetting) {
        plan.calculate_subsets(&fonts.font_set, webroot.map(|x| &*x.0))?
    } else {
        AssignedSubsets::disabled().clone()
    });

    let mut joins = JoinSet::new();
    for font in fonts.font_set.as_list() {
        if plan.family_config.check_font(&font) {
            let plan = plan.clone();
            let assigned = assigned.clone();
            let font = font.clone();

            let span = info_span!("split", "{font}");
            let _enter = span.enter();

            joins.spawn(
                async move { Ok(vec![splitter::split_webfont(&plan, &assigned, &font).await?]) }
                    .in_current_span(),
            );
        } else {
            info!("Font family is excluded: {}", font)
        }
    }
    {
        let span = info_span!("split", "(fallback font)");
        let _enter = span.enter();
        let assigned = assigned.clone();
        joins.spawn(
            async move { splitter::make_fallback_font(&plan, &assigned).await }.in_current_span(),
        );
    }

    let webfonts = joins.join_vec().await?.into_iter().map(Arc::new).collect();
    Ok(WebfontResults {
        webfonts,
        fallback_info: assigned.get_fallback_info().clone(),
        webroot: webroot.cloned(),
    })
}
