use anyhow::*;
use roaring::RoaringBitmap;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ops::RangeInclusive,
    sync::Arc,
};

#[derive(Clone, Debug)]
pub struct WebfontData {
    pub by_name: HashMap<Arc<str>, Arc<WebfontSubset>>,
    pub subsets: Vec<Arc<WebfontSubset>>,
    pub groups: Vec<Arc<WebfontSubsetGroup>>,
}
#[derive(Clone, Debug)]
pub struct WebfontSubsetGroup {
    pub name: Arc<str>,
    pub subsets: Vec<Arc<WebfontSubset>>,
}
#[derive(Clone, Debug)]
pub struct WebfontSubset {
    pub name: Arc<str>,
    pub map: RoaringBitmap,
}

impl WebfontData {
    pub fn load(data: &str) -> Result<WebfontData> {
        #[derive(Deserialize)]
        struct RawSubset {
            name: String,
            group: Option<String>,
            chars: String,
            codepoints: Vec<u32>,
        }

        #[derive(Deserialize)]
        struct RawSubsets {
            subset: Vec<RawSubset>,
        }

        let loaded_data: RawSubsets = toml::from_str(data)?;
        let mut loaded_data = loaded_data.subset;
        loaded_data.sort_by_cached_key(|x| x.name.clone());

        let mut data = WebfontData { by_name: Default::default(), subsets: vec![], groups: vec![] };

        let mut groups_tmp: BTreeMap<_, Vec<_>> = BTreeMap::new();
        let mut present_names = HashSet::new();
        for raw in loaded_data {
            let name: Arc<str> = raw.name.into();

            if present_names.contains(&name) {
                bail!("Duplicate subset name: {}", name);
            }
            present_names.insert(name.clone());

            let mut map = RoaringBitmap::new();
            for ch in raw.chars.chars() {
                map.insert(ch as u32);
            }
            for ch in raw.codepoints {
                map.insert(ch);
            }

            let subset = Arc::new(WebfontSubset { name: name.clone(), map });
            match raw.group {
                None => data.subsets.push(subset.clone()),
                Some(group) => groups_tmp.entry(group).or_default().push(subset.clone()),
            }
            data.by_name.insert(name, subset);
        }
        for (k, v) in groups_tmp {
            data.groups
                .push(Arc::new(WebfontSubsetGroup { name: k.into(), subsets: v }));
        }

        Ok(data)
    }
}

//noinspection DuplicatedCode
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
