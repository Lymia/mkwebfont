use crate::{
    plan::{AssignedSubsets, FontFlags, LoadedSplitterPlan, SubsetDataBuilder},
    WebfontInfo,
};
use anyhow::Result;
use mkwebfont_common::join_set::JoinSet;
use mkwebfont_fontops::{
    font_info::{FontFaceSet, FontFaceWrapper},
    gfonts::fallback_info::FallbackInfo,
    subsetter::FontEncoder,
};
use std::sync::Arc;
use tracing::{info, info_span};
use tracing_futures::Instrument;

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
    let mut encoder = FontEncoder::new(font.clone(), assigned.get_range_exclusion(font));

    if !assigned.get_used_chars(font).is_empty() {
        if plan.flags.contains(FontFlags::NoSplitter) {
            NullSplitter
                .split(font, plan, assigned, &mut encoder)
                .await?
        } else if plan.flags.contains(FontFlags::GfontsSplitter) {
            gfsubsets::GfSubsetSplitter
                .split(font, plan, assigned, &mut encoder)
                .await?
        } else {
            unreachable!()
        }
    }

    let info = encoder.produce_webfont().await?;
    let codepoints = font.all_codepoints().len();
    let subsets = info.subsets().len();
    let remaining_codepoints = assigned.get_used_chars(font).len();
    if codepoints == remaining_codepoints {
        info!("Split {codepoints} codepoints into {subsets} subsets!");
    } else {
        info!(
            "Split {remaining_codepoints} codepoints into {subsets} subsets! \
             ({codepoints} codepoints before subsetting)"
        );
    }
    anyhow::Ok(info)
}

pub const FALLBACK_FONT_NAME: &str = "mkwebfontFallbackV1";

pub async fn make_fallback_font(
    plan: &LoadedSplitterPlan,
    assigned: &AssignedSubsets,
) -> Result<Vec<WebfontInfo>> {
    let chars = assigned.get_fallback_chars().clone();
    if chars.is_empty() {
        Ok(Vec::new())
    } else {
        let needed_fonts = FallbackInfo::load_needed_fonts(&chars).await?;
        let font_set = FontFaceSet::build(needed_fonts.into_iter());
        let fallback_stack = FallbackInfo::build_stack(&chars);

        let mut assigned = SubsetDataBuilder::default();
        let mut stack_fonts = Vec::new();
        for font in fallback_stack {
            stack_fonts.push(font_set.resolve_all(&font)?);
        }
        assigned.push_stack(chars.clone(), &stack_fonts)?;
        let assigned = Arc::new(assigned.build());

        let mut joins = JoinSet::new();
        for font in font_set.as_list() {
            let assigned = assigned.clone();
            let chars = chars.clone();
            let font = font.clone();
            let plan = plan.clone();

            let name = font.font_family().to_string();

            joins.spawn(
                async move {
                    let mut encoder = FontEncoder::new(font.clone(), chars);

                    gfsubsets::GfSubsetSplitter
                        .split(&font, &plan, &*assigned, &mut encoder)
                        .await?;
                    let info = encoder
                        .produce_webfont()
                        .await?
                        .with_family_name(FALLBACK_FONT_NAME);

                    let codepoints = font.all_codepoints().len();
                    let subsets = info.subsets().len();
                    let remaining_codepoints = assigned.get_used_chars(&font).len();
                    info!(
                        "Split {remaining_codepoints} codepoints into {subsets} subsets! \
                         ({codepoints} codepoints before subsetting)"
                    );

                    Ok(info)
                }
                .instrument(info_span!("split", "{name}")),
            );
        }
        joins.join().await
    }
}
