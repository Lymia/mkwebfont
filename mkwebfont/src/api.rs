use crate::{fonts::FontFaceWrapper, splitter};
use anyhow::Result;
use mkwebfont_common::join_set::JoinSet;
use roaring::RoaringBitmap;
use std::{collections::HashSet, path::Path};
use tokio::{sync::Mutex, task::JoinHandle};
use tracing::{debug, info, info_span, warn, Instrument};

pub use crate::{
    render::{SubsetInfo, WebfontInfo},
    subset_plan::SubsetPlan,
};

/// A loaded font.
///
/// This may be used to filter font collections or simply subset multiple fonts in one operation.
pub struct LoadedFont {
    underlying: FontFaceWrapper,
}
impl LoadedFont {
    /// Loads all fonts present in a given binary font data.
    pub fn load(font_data: &[u8]) -> Result<Vec<Self>> {
        Ok(FontFaceWrapper::load(font_data.into())?
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

const FINISH_PRELOAD: Mutex<Vec<JoinHandle<Result<()>>>> = Mutex::const_new(Vec::new());
async fn finish_preload() -> Result<()> {
    for join in FINISH_PRELOAD.lock().await.drain(..) {
        join.await??;
    }
    Ok(())
}

impl SubsetPlan {
    /// Preload resources required for this subsetting plan.
    pub async fn preload(&self) -> Result<()> {
        warn!("(TODO: preload called)");
        Ok(())
    }
}

/// A fast function for loading fonts from disk.
pub async fn load_fonts_from_disk(
    paths: impl IntoIterator<Item = impl AsRef<Path>>,
) -> Result<Vec<LoadedFont>> {
    let mut joins = JoinSet::new();
    for path in paths {
        let path = path.as_ref().to_path_buf();
        joins.spawn(async move {
            info!("Loading fonts: {}", path.display());
            LoadedFont::load(&std::fs::read(path)?)
        });
    }

    let fonts = joins.join_vec().await?;
    info!("Loaded {} font families...", fonts.len());
    Ok(fonts)
}

pub async fn process_webfont(plan: &SubsetPlan, fonts: &[LoadedFont]) -> Result<Vec<WebfontInfo>> {
    let plan = plan.build();

    finish_preload().await?;

    let mut awaits = Vec::new();
    for font in fonts {
        if plan.family_config.check_font(&font.underlying) {
            let plan = plan.clone();
            let font = font.underlying.clone();

            let span = info_span!("split", "{font}");
            let _enter = span.enter();

            awaits.push(tokio::task::spawn(
                async move { splitter::split_webfont(&plan, &font).await }.in_current_span(),
            ));
        } else {
            info!("Font family is excluded: {}", font.underlying)
        }
    }

    let mut out = Vec::new();
    for font in awaits {
        out.push(font.await??)
    }
    Ok(out)
}
