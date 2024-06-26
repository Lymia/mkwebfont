use crate::model::data_package::{DataSection, DataSectionEncoder};
use bincode::{Decode, Encode};
use roaring::RoaringBitmap;
use std::{collections::HashMap, sync::Arc};

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

#[derive(Clone, Debug, Encode, Decode)]
pub struct RawSubset {
    pub name: String,
    pub group: Option<String>,
    pub chars: String,
}

#[derive(Clone, Debug, Encode, Decode)]
pub struct RawSubsets {
    pub subsets: Vec<RawSubset>,
}

fn convert_subset(name: &str, chars: &str) -> Arc<WebfontSubset> {
    let mut bitmap = RoaringBitmap::new();
    for ch in chars.chars() {
        bitmap.insert(ch as u32);
    }
    Arc::new(WebfontSubset { name: name.into(), map: bitmap })
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

pub fn build_from_table(table: HashMap<String, String>) -> WebfontData {
    let subsets: Vec<_> = table
        .into_iter()
        .map(|(k, v)| convert_subset(&k, &v))
        .collect();
    let by_name = build_by_name(&subsets);
    WebfontData { by_name, subsets, groups: vec![] }
}

impl RawSubsets {
    pub fn build(&self) -> WebfontData {
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

/// Serialization code
impl RawSubsets {
    const TYPE_TAG: &'static str = "RawSubsets";

    pub fn serialize(self, tag: &str) -> anyhow::Result<DataSection> {
        let mut encoder = DataSectionEncoder::new(tag, Self::TYPE_TAG);
        encoder.insert_bincode("*", &self);
        Ok(encoder.build())
    }

    pub fn deserialize(mut section: DataSection) -> anyhow::Result<Self> {
        section.type_check(Self::TYPE_TAG)?;
        Ok(section.take_bincode("*")?)
    }
}
