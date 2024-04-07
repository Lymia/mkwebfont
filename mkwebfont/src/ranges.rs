use crate::gf_ranges::{GfSubset, GfSubsets};
use lazy_static::lazy_static;
use roaring::RoaringBitmap;
use std::{collections::HashMap, ops::RangeInclusive};

pub struct WebfontDataCtx {
    pub by_name: HashMap<&'static str, WebfontSubset>,
    pub subsets: Vec<WebfontSubset>,
    pub groups: Vec<WebfontSubsetGroup>,
}
impl WebfontDataCtx {
    pub fn load() -> &'static WebfontDataCtx {
        &CTX
    }
}

pub struct WebfontSubsetGroup {
    pub name: &'static str,
    pub subsets: Vec<WebfontSubset>,
}
pub struct WebfontSubset {
    pub name: &'static str,
    pub map: RoaringBitmap,
}

fn load_subset(subset: &GfSubset) -> WebfontSubset {
    WebfontSubset { name: subset.name, map: encode_range(subset.ranges) }
}
fn load_list(subsets: &[GfSubset]) -> Vec<WebfontSubset> {
    subsets.iter().map(load_subset).collect()
}
fn load() -> WebfontDataCtx {
    let mut by_name = HashMap::new();
    let mut groups = Vec::new();

    for subset in GfSubsets::DATA.subsets {
        by_name.insert(subset.name, load_subset(subset));
    }
    for group in GfSubsets::DATA.subset_groups {
        groups.push(WebfontSubsetGroup { name: group.name, subsets: load_list(group.subsets) });
        for subset in group.subsets {
            by_name.insert(subset.name, load_subset(subset));
        }
    }

    WebfontDataCtx { by_name, subsets: load_list(GfSubsets::DATA.subsets), groups }
}

lazy_static! {
    static ref CTX: WebfontDataCtx = load();
}

pub fn encode_range(ranges: &[RangeInclusive<char>]) -> RoaringBitmap {
    let mut bitmap = RoaringBitmap::new();
    for range in ranges {
        for char in range.clone() {
            bitmap.insert(char as u32);
        }
    }
    bitmap
}
pub fn decode_range(bitmap: &RoaringBitmap) -> Vec<RangeInclusive<char>> {
    let mut range_start = None;
    let mut range_last = '\u{fffff}';
    let mut ranges = Vec::new();
    for char in bitmap {
        let char = char::from_u32(char).expect("Invalid char in RoaringBitmap");
        if let Some(start) = range_start {
            let next = char::from_u32(range_last as u32 + 1).unwrap();
            if next != char {
                ranges.push(start..=range_last);
                range_start = Some(char);
            }
        } else {
            range_start = Some(char);
        }
        range_last = char;
    }
    if let Some(start) = range_start {
        ranges.push(start..=range_last);
    }
    ranges
}
