use roaring::RoaringBitmap;
use std::{ops::Deref, sync::Arc};

#[derive(Clone)]
pub struct SubsetPlan(pub Arc<SubsetPlanData>);
impl Deref for SubsetPlan {
    type Target = SubsetPlanData;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
pub struct SubsetPlanData {
    /// A set of characters that should be injected into the same font as the basic latin
    /// characters. This is meant for use with common UI elements used across a website.
    pub preload: RoaringBitmap,
}

pub struct SubsetPlanBuilder {
    preload: RoaringBitmap,
}
impl SubsetPlanBuilder {
    pub fn new() -> SubsetPlanBuilder {
        SubsetPlanBuilder { preload: Default::default() }
    }

    pub fn preload_chars(&mut self, chars: impl Iterator<Item = char>) -> &mut Self {
        for ch in chars {
            self.preload.insert(ch as u32);
        }
        self
    }

    pub(crate) fn build(&self) -> SubsetPlan {
        SubsetPlan(Arc::new(SubsetPlanData { preload: self.preload.clone() }))
    }
}
