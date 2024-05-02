mod data;
mod fonts;
mod render;
mod splitter;
mod subset_plan;

pub use render::{SubsetInfo, WebfontInfo};
pub use subset_plan::SubsetPlanBuilder as SubsetPlan;

/// A loaded font.
///
/// This may be used to filter font collections or simply subset multiple fonts in one operation.
pub struct LoadedFont {
    underlying: fonts::LoadedFont,
}
impl LoadedFont {
    /// Loads all fonts present in a given binary font data.
    pub fn load(font_data: &[u8]) -> anyhow::Result<Vec<Self>> {
        Ok(fonts::LoadedFont::load(font_data.into())?
            .into_iter()
            .map(|x| LoadedFont { underlying: x })
            .collect())
    }

    /// Returns the list of codepoints in the loaded font.
    pub fn codepoints(&self) -> roaring::RoaringBitmap {
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

pub async fn process_webfont(
    plan: &SubsetPlan,
    fonts: &[LoadedFont],
) -> anyhow::Result<Vec<WebfontInfo>> {
    use tracing::Instrument;

    let plan = plan.build();

    let mut awaits = Vec::new();
    for font in fonts {
        let plan = plan.clone();
        let font = font.underlying.clone();

        let span = tracing::info_span!("split", "{font}");
        let _enter = span.enter();

        awaits.push(tokio::task::spawn(
            async move { splitter::split_webfont(&plan, &font).await }.in_current_span(),
        ));
    }

    let mut out = Vec::new();
    for font in awaits {
        out.push(font.await??)
    }
    Ok(out)
}
