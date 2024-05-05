use crate::{
    fonts::FontFaceWrapper,
    render::FontEncoder,
    subset_plan::{FontFlags, LoadedSubsetPlan},
    WebfontInfo,
};
use anyhow::Result;
use tracing::info;

mod adjacency;
mod gfsubsets;

pub trait SplitterImplementation {
    async fn split(
        &self,
        font: &FontFaceWrapper,
        plan: &LoadedSubsetPlan,
        encoder: &mut FontEncoder,
    ) -> Result<()>;
}

struct NullSplitter;
impl SplitterImplementation for NullSplitter {
    async fn split(
        &self,
        font: &FontFaceWrapper,
        _: &LoadedSubsetPlan,
        encoder: &mut FontEncoder,
    ) -> Result<()> {
        encoder.add_subset("all", font.all_codepoints().clone());
        Ok(())
    }
}

/// The internal function that actually splits the webfont.
pub async fn split_webfont(plan: &LoadedSubsetPlan, font: &FontFaceWrapper) -> Result<WebfontInfo> {
    let mut encoder = FontEncoder::new(font.clone());

    if plan.flags.contains(FontFlags::NoSplitter) {
        NullSplitter.split(font, plan, &mut encoder).await?
    } else if plan.flags.contains(FontFlags::AdjacencySplitter) {
        adjacency::AdjacencySplitter
            .split(font, plan, &mut encoder)
            .await?
    } else if plan.flags.contains(FontFlags::GfsubsetSplitter) {
        gfsubsets::GfSubsetSplitter
            .split(font, plan, &mut encoder)
            .await?
    } else {
        unreachable!()
    }

    let info = encoder.produce_webfont().await?;
    info!(
        "Successfully split {} codepoints into {} subsets!",
        font.all_codepoints().len(),
        info.subsets().len(),
    );
    anyhow::Ok(info)
}
