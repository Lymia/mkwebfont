use crate::{
    fonts::FontFaceWrapper,
    render::FontEncoder,
    splitter_plan::{FontFlags, LoadedSplitterPlan},
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
        plan: &LoadedSplitterPlan,
        encoder: &mut FontEncoder,
    ) -> Result<()>;
}

struct NullSplitter;
impl SplitterImplementation for NullSplitter {
    async fn split(
        &self,
        font: &FontFaceWrapper,
        plan: &LoadedSplitterPlan,
        encoder: &mut FontEncoder,
    ) -> Result<()> {
        encoder.add_subset("all", plan, plan.do_split(font.all_codepoints().clone()));
        Ok(())
    }
}

/// The internal function that actually splits the webfont.
pub async fn split_webfont(
    plan: &LoadedSplitterPlan,
    font: &FontFaceWrapper,
) -> Result<WebfontInfo> {
    let mut encoder = FontEncoder::new(font.clone());

    if plan.flags.contains(FontFlags::NoSplitter) {
        NullSplitter.split(font, plan, &mut encoder).await?
    } else if plan.flags.contains(FontFlags::AdjacencySplitter) {
        adjacency::AdjacencySplitter
            .split(font, plan, &mut encoder)
            .await?
    } else if plan.flags.contains(FontFlags::GfontsSplitter) {
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
