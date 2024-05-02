use crate::{fonts::LoadedFont, render::FontEncoder, subset_plan::SubsetPlan, WebfontInfo};
use anyhow::Result;
use tracing::info;

mod gfsubsets;

pub trait SplitterImplementation {
    async fn split(
        &self,
        font: &LoadedFont,
        plan: &SubsetPlan,
        encoder: &mut FontEncoder,
    ) -> Result<()>;
}

/// The internal function that actually splits the webfont.
pub async fn split_webfont(plan: &SubsetPlan, font: &LoadedFont) -> Result<WebfontInfo> {
    let mut encoder = FontEncoder::new(font.clone());
    gfsubsets::GfSubsetSplitter
        .split(font, plan, &mut encoder)
        .await?;

    let info = encoder.produce_webfont().await?;
    info!(
        "Successfully split {} codepoints into {} subsets!",
        font.all_codepoints().len(),
        info.subsets().len(),
    );
    anyhow::Ok(info)
}
