use crate::{
    plan::{AssignedSubsets, FontFlags, LoadedSplitterPlan},
    WebfontInfo,
};
use anyhow::Result;
use mkwebfont_fontops::{font_info::FontFaceWrapper, subsetter::FontEncoder};
use tracing::info;

mod adjacency;
mod gfsubsets;

pub trait SplitterImplementation {
    async fn split(
        &self,
        font: &FontFaceWrapper,
        plan: &LoadedSplitterPlan,
        assigned: &AssignedSubsets,
        encoder: &mut FontEncoder,
    ) -> Result<()>;
}

struct NullSplitter;
impl SplitterImplementation for NullSplitter {
    async fn split(
        &self,
        font: &FontFaceWrapper,
        _plan: &LoadedSplitterPlan,
        assigned: &AssignedSubsets,
        encoder: &mut FontEncoder,
    ) -> Result<()> {
        encoder.add_subset("all", assigned.get_used_chars(font));
        Ok(())
    }
}

/// The internal function that actually splits the webfont.
pub async fn split_webfont(
    plan: &LoadedSplitterPlan,
    assigned: &AssignedSubsets,
    font: &FontFaceWrapper,
) -> Result<WebfontInfo> {
    let mut encoder = FontEncoder::new(font.clone());

    if plan.flags.contains(FontFlags::NoSplitter) {
        NullSplitter
            .split(font, plan, assigned, &mut encoder)
            .await?
    } else if plan.flags.contains(FontFlags::AdjacencySplitter) {
        adjacency::AdjacencySplitter
            .split(font, plan, assigned, &mut encoder)
            .await?
    } else if plan.flags.contains(FontFlags::GfontsSplitter) {
        gfsubsets::GfSubsetSplitter
            .split(font, plan, assigned, &mut encoder)
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
