use crate::{data::DataStorage, plan::FontFlags, quality_report::FontReport, splitter};
use anyhow::Result;
use mkwebfont_common::join_set::JoinSet;
use mkwebfont_fontops::font_info::{FontFaceSet, FontFaceWrapper};
use roaring::RoaringBitmap;
use std::path::Path;
use tokio::{sync::Mutex, task::JoinHandle};
use tracing::{debug, info, info_span, Instrument};

pub use crate::plan::SplitterPlan;
pub use mkwebfont_fontops::{
    font_info::{FontStyle, FontWeight},
    subsetter::{SubsetInfo, WebfontInfo},
};

/// A loaded font.
///
/// This may be used to filter font collections or simply subset multiple fonts in one operation.
#[derive(Clone)]
pub struct LoadedFont {
    underlying: FontFaceWrapper,
}
impl LoadedFont {
    /// Loads all fonts present in a given binary font data.
    pub fn load(font_data: &[u8]) -> Result<Vec<Self>> {
        Ok(FontFaceWrapper::load(None, font_data.into())?
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
    pub fn codepoints(&self) -> RoaringBitmap {
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
}
impl LoadedFontSetBuilder {
    /// Creates a new empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// A fast function for loading fonts from disk.
    pub async fn load_from_disk(paths: impl IntoIterator<Item = impl AsRef<Path>>) -> Result<Self> {
        let mut set = Self::new();
        for font in load_fonts_from_disk(paths).await? {
            set = set.add_font(font);
        }
        Ok(set)
    }

    /// Merges two font set builders.
    pub fn merge(mut self, other: LoadedFontSetBuilder) {
        self.fonts.extend(other.fonts);
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

    /// Builds the final font set.
    pub fn build(self) -> LoadedFontSet {
        LoadedFontSet { font_set: FontFaceSet::build(self.fonts.into_iter().map(|x| x.underlying)) }
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
    pub fn resolve(&self, name: &str) -> Result<LoadedFont> {
        Ok(LoadedFont { underlying: self.font_set.resolve(name)?.clone() })
    }
}

/// A fast function for loading fonts from disk.
async fn load_fonts_from_disk(
    paths: impl IntoIterator<Item = impl AsRef<Path>>,
) -> Result<Vec<LoadedFont>> {
    let mut joins = JoinSet::new();
    for path in paths {
        let path = path.as_ref().to_path_buf();
        joins.spawn(async move {
            info!("Loading fonts: {}", path.display());
            LoadedFont::load_path(&path)
        });
    }

    let fonts = joins.join_vec().await?;
    info!("Loaded {} font families...", fonts.len());
    Ok(fonts)
}

/// Helper for preloading.
const FINISH_PRELOAD: Mutex<Vec<JoinHandle<Result<()>>>> = Mutex::const_new(Vec::new());
async fn finish_preload() -> Result<()> {
    for join in FINISH_PRELOAD.lock().await.drain(..) {
        join.await??;
    }
    Ok(())
}

impl SplitterPlan {
    /// Preload resources required for this subsetting plan.
    pub async fn preload(&self) -> Result<()> {
        let span = info_span!("preload");
        let _enter = span.enter();
        if self.flags.contains(FontFlags::PrintReport) {
            FINISH_PRELOAD.lock().await.push(tokio::spawn(
                async {
                    debug!("Preloading validation list...");
                    DataStorage::instance()?.validation_list().await?;
                    Ok(())
                }
                .in_current_span(),
            ));
        }
        if self.flags.contains(FontFlags::GfontsSplitter) {
            FINISH_PRELOAD.lock().await.push(tokio::spawn(
                async {
                    debug!("Preloading gfsubsets...");
                    DataStorage::instance()?.gfsubsets().await?;
                    Ok(())
                }
                .in_current_span(),
            ));
        }
        if self.flags.contains(FontFlags::AdjacencySplitter) {
            FINISH_PRELOAD.lock().await.push(tokio::spawn(
                async {
                    debug!("Preloading adjacency list...");
                    DataStorage::instance()?.adjacency_array().await?;
                    Ok(())
                }
                .in_current_span(),
            ));
        }
        Ok(())
    }
}

pub async fn process_webfont(
    plan: &SplitterPlan,
    fonts: &LoadedFontSet,
) -> Result<Vec<WebfontInfo>> {
    let plan = plan.build();

    finish_preload().await?;

    let mut joins = JoinSet::new();
    for font in fonts.font_set.as_list() {
        if plan.family_config.check_font(&font) {
            let plan = plan.clone();
            let font = font.clone();

            let span = info_span!("split", "{font}");
            let _enter = span.enter();

            joins.spawn(
                async move {
                    let font = splitter::split_webfont(&plan, &font).await?;
                    let report = if plan.flags.contains(FontFlags::PrintReport) {
                        Some(FontReport::for_font(&font).await?)
                    } else {
                        None
                    };
                    Ok((font, report))
                }
                .in_current_span(),
            );
        } else {
            info!("Font family is excluded: {}", font)
        }
    }

    let mut out = Vec::new();
    for (font, report) in joins.join().await? {
        out.push(font);
        if let Some(report) = report {
            report.print();
        }
        eprintln!();
    }
    Ok(out)
}
