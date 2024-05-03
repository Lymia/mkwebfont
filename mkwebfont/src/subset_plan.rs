use roaring::RoaringBitmap;
use std::{ops::Deref, sync::Arc};

#[derive(Clone)]
pub struct ParsedSubsetPlan(pub Arc<SubsetPlanData>);
impl Deref for ParsedSubsetPlan {
    type Target = SubsetPlanData;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
pub struct SubsetPlanData {
    pub preload: RoaringBitmap,
}

/// Represents a configuration for subsetting.
#[derive(Clone, Debug)]
pub struct SubsetPlan {
    preload: RoaringBitmap,
}
impl SubsetPlan {
    pub fn new() -> SubsetPlan {
        SubsetPlan { preload: Default::default() }
    }

    /// A set of characters that should be injected into the same font as the basic latin
    /// characters. This is meant for use with common UI elements used across a website.
    pub fn preload_chars(&mut self, chars: impl Iterator<Item = char>) -> &mut Self {
        for ch in chars {
            self.preload.insert(ch as u32);
        }
        self
    }

    pub(crate) fn build(&self) -> ParsedSubsetPlan {
        ParsedSubsetPlan(Arc::new(SubsetPlanData { preload: self.preload.clone() }))
    }
}
