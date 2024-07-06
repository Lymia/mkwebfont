use crate::{
    character_set::{CharacterSet, CompressedCharacterSet},
    model::data_package::{DataSection, DataSectionEncoder},
};
use anyhow::Result;
use bincode::{Decode, Encode};
use std::sync::Arc;

pub struct BitsetSectionBuilder {
    source: String,
    compressed: Vec<CompressedCharacterSet>,

    used_cd_idx: u8,
    used_cd: Box<[u8; 0x110000]>,
    filtered: Box<[bool; 0x110000]>,
}
impl BitsetSectionBuilder {
    pub fn new(source: &str) -> Self {
        BitsetSectionBuilder {
            source: source.to_string(),
            compressed: vec![],
            used_cd_idx: 0,
            used_cd: Box::new([0; 0x110000]),
            filtered: Box::new([false; 0x110000]),
        }
    }

    pub fn filter_chars(&mut self, filter: impl Fn(char) -> bool) {
        for i in 0..self.filtered.len() {
            if let Some(ch) = char::from_u32(i as u32) {
                if !filter(ch) {
                    self.filtered[i] = true;
                }
            } else {
                self.filtered[i] = true;
            }
        }
    }

    pub fn push_sample(&mut self, str: &str) {
        let idx = self.used_cd_idx;
        if idx == u8::MAX {
            self.used_cd_idx = 0;
            *self.used_cd = [0; 0x110000];
        } else {
            self.used_cd_idx += 1;
        }

        let mut bitset = CharacterSet::new();
        for ch in str.chars() {
            if (ch as u32) < 0x110000 {
                if self.used_cd[ch as usize] != idx {
                    self.used_cd[ch as usize] = idx;
                } else {
                    continue;
                }

                if !self.filtered[ch as usize] {
                    bitset.insert(ch as u32);
                }
            }
        }
        self.compressed.push(bitset.compressed());
    }
}

pub fn build(raw_sections: Vec<BitsetSectionBuilder>) -> BitsetList {
    let mut sections = Vec::new();
    for section in raw_sections {
        sections.push(Arc::new(BitsetSection {
            source: section.source,
            compressed: section.compressed,
        }));
    }
    BitsetList { sections }
}

#[derive(Clone, Decode, Encode)]
pub struct BitsetSection {
    source: String,
    compressed: Vec<CompressedCharacterSet>,
}
impl BitsetSection {
    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn len(&self) -> usize {
        self.compressed.len()
    }

    pub fn iter<'a>(&'a self) -> impl Iterator<Item = CharacterSet> + 'a {
        self.compressed.iter().map(|x| CharacterSet::decompress(x))
    }

    pub fn get(&self, i: usize) -> CharacterSet {
        CharacterSet::decompress(&self.compressed[i])
    }
}

#[derive(Clone, Decode, Encode)]
pub struct BitsetList {
    sections: Vec<Arc<BitsetSection>>,
}
impl BitsetList {
    pub fn len(&self) -> u64 {
        let mut count = 0;
        for section in &self.sections {
            count += section.len() as u64;
        }
        count
    }

    pub fn sections(&self) -> &[Arc<BitsetSection>] {
        &self.sections
    }

    pub fn take_sections(self) -> Vec<Arc<BitsetSection>> {
        self.sections
    }

    pub fn iter(&self) -> impl Iterator<Item = CharacterSet> + '_ {
        self.sections.iter().flat_map(|x| x.iter())
    }

    pub fn split(&self, count: usize) -> Vec<BitsetList> {
        let mut sections = vec![Vec::new(); count];
        for (i, section) in self.sections.iter().enumerate() {
            sections[i % count].push(section.clone());
        }
        sections
            .into_iter()
            .map(|x| BitsetList { sections: x })
            .collect()
    }
}

/// Serialization code
impl BitsetList {
    const TYPE_TAG: &'static str = "BitsetList/2.0";

    pub fn serialize(self, tag: &str) -> Result<DataSection> {
        let mut encoder = DataSectionEncoder::new(tag, Self::TYPE_TAG);
        encoder.insert_bincode("*", &self);
        Ok(encoder.build())
    }

    pub fn deserialize(mut section: DataSection) -> Result<Self> {
        section.type_check(Self::TYPE_TAG)?;
        Ok(section.take_bincode("*")?)
    }
}
