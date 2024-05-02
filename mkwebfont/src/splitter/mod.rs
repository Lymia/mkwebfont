use crate::{fonts::LoadedFont, render::FontEncoder, subset_plan::SubsetPlan};
use anyhow::Result;

mod gfsubsets;

pub trait SplitterImplementation {
    async fn split(
        &self,
        font: &LoadedFont,
        plan: &SubsetPlan,
        encoder: &mut FontEncoder,
    ) -> Result<()>;
}
