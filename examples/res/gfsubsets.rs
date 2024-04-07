
// --- static from gfsubsets.rs --- //
use std::ops::RangeInclusive;

#[derive(Debug)]
pub struct GfSubsets {
    pub subsets: &'static [GfSubset],
    pub subset_groups: &'static [GfSubsetGroup],
}

#[derive(Debug)]
pub struct GfSubsetGroup {
    pub name: &'static str,
    pub subsets: &'static [GfSubset],
}

#[derive(Debug)]
pub struct GfSubset {
    pub name: &'static str,
    pub ranges: &'static [RangeInclusive<char>],
}

const fn o(c: char) -> RangeInclusive<char> {
    c..=c
}
// --- end static from gfsubsets.rs --- //
