use bincode::{config::standard, Decode, Encode};
use mkwebfont_common::{
    character_set::{CharacterSet, CompressedCharacterSet},
    compression::zstd_decompress,
};
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};

#[derive(Clone, Debug)]
pub struct WebfontData {
    pub by_name: HashMap<Arc<str>, Arc<WebfontSubset>>,
    pub subsets: Vec<Arc<WebfontSubset>>,
    pub groups: Vec<Arc<WebfontSubsetGroup>>,
}
impl WebfontData {
    pub fn load<'a>() -> &'a WebfontData {
        static CACHE: LazyLock<WebfontData> = LazyLock::new(|| {
            let data = include_bytes!("gfonts_subsets.bin.zst");
            let decompressed = zstd_decompress(data).unwrap();
            let out: RawSubsets = bincode::decode_from_slice(&decompressed, standard())
                .unwrap()
                .0;
            out.build()
        });
        &*CACHE
    }
}

#[derive(Clone, Debug)]
pub struct WebfontSubsetGroup {
    pub name: Arc<str>,
    pub subsets: Vec<Arc<WebfontSubset>>,
}

#[derive(Clone, Debug)]
pub struct WebfontSubset {
    pub name: Arc<str>,
    pub map: CharacterSet,
}

#[derive(Clone, Debug, Encode, Decode)]
pub struct RawSubset {
    pub name: String,
    pub group: Option<String>,
    pub chars: CompressedCharacterSet,
}

#[derive(Clone, Debug, Encode, Decode)]
pub struct RawSubsets {
    pub subsets: Vec<RawSubset>,
}

fn convert_subset(name: &str, chars: &CompressedCharacterSet) -> Arc<WebfontSubset> {
    Arc::new(WebfontSubset { name: name.into(), map: CharacterSet::decompress(chars) })
}
fn build_by_name(subsets: &[Arc<WebfontSubset>]) -> HashMap<Arc<str>, Arc<WebfontSubset>> {
    let mut by_name = HashMap::new();
    for subset in subsets {
        by_name.insert(subset.name.clone(), subset.clone());
    }
    by_name
}
fn split_groups(
    group_names: &HashMap<String, String>,
    subsets: Vec<Arc<WebfontSubset>>,
) -> (Vec<Arc<WebfontSubset>>, Vec<Arc<WebfontSubsetGroup>>) {
    let (has_group, no_group) = subsets
        .into_iter()
        .partition::<Vec<_>, _>(|x| group_names.contains_key(x.name.as_ref()));
    let mut groups: HashMap<_, Vec<_>> = HashMap::new();
    for group in has_group {
        groups
            .entry(group_names.get(group.name.as_ref()).unwrap().clone())
            .or_default()
            .push(group);
    }
    let groups: Vec<_> = groups
        .into_iter()
        .map(|(k, v)| Arc::new(WebfontSubsetGroup { name: k.into(), subsets: v }))
        .collect();
    (no_group, groups)
}

impl RawSubsets {
    fn build(&self) -> WebfontData {
        let groups: HashMap<_, _> = self
            .subsets
            .iter()
            .flat_map(|v| v.group.as_ref().map(|g| (v.name.clone(), g.clone())))
            .collect();
        let subsets: Vec<_> = self
            .subsets
            .iter()
            .map(|v| convert_subset(&v.name, &v.chars))
            .collect();
        let by_name = build_by_name(&subsets);
        let (subsets, groups) = split_groups(&groups, subsets);
        WebfontData { by_name, subsets, groups }
    }
}
