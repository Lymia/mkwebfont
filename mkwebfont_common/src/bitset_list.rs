use crate::{
    data_package::{DataPackage, DataPackageEncoder},
    wyhash::WyHashBuilder,
};
use anyhow::Result;
use bincode::{config, Decode, Encode};
use roaring::RoaringBitmap;
use std::{collections::HashMap, io::Cursor};

pub struct BitsetListBuilder {
    codepoint_list: Vec<u32>,
    glyph_mapping: HashMap<u32, u32, WyHashBuilder>,
    index: Vec<usize>,
    data: Vec<u8>,

    used_cd_idx: u16,
    used_cd: Box<[u16; 0x110000]>,
}
impl BitsetListBuilder {
    pub fn new() -> Self {
        BitsetListBuilder {
            codepoint_list: vec![],
            glyph_mapping: Default::default(),
            index: vec![],
            data: vec![],
            used_cd_idx: 0,
            used_cd: Box::new([0; 0x110000]),
        }
    }

    fn map_ch(&mut self, ch: u32) -> u32 {
        if let Some(&idx) = self.glyph_mapping.get(&ch) {
            idx
        } else {
            let idx = self.glyph_mapping.len() as u32;
            self.glyph_mapping.insert(ch, idx);
            self.codepoint_list.push(ch);
            idx
        }
    }

    pub fn push_sample(&mut self, str: &str, filter: impl Fn(char) -> bool) {
        let idx = self.used_cd_idx;
        if idx == u16::MAX {
            self.used_cd_idx = 0;
            *self.used_cd = [0; 0x110000];
        } else {
            self.used_cd_idx += 1;
        }

        let mut bitset = RoaringBitmap::new();
        for ch in str.chars() {
            if (ch as u32) < 0x110000 {
                if self.used_cd[ch as usize] != idx {
                    self.used_cd[ch as usize] = idx;
                }
                if filter(ch) {
                    bitset.insert(self.map_ch(ch as u32));
                }
            }
        }

        self.index.push(self.data.len());
        bitset.serialize_into(Cursor::new(&mut self.data)).unwrap();
    }
}

pub fn build(raw_sections: Vec<BitsetListBuilder>) -> BitsetList {
    let mut sections = Vec::new();
    for section in raw_sections {
        sections.push(BitsetSection {
            index: section.index,
            codepoint_list: section.codepoint_list,
            data: section.data,
        });
    }
    BitsetList { sections }
}

#[derive(Clone, Decode, Encode)]
struct BitsetSection {
    index: Vec<usize>,
    codepoint_list: Vec<u32>,
    data: Vec<u8>,
}

#[derive(Clone, Decode, Encode)]
pub struct BitsetList {
    sections: Vec<BitsetSection>,
}
impl BitsetList {
    pub fn len(&self) -> u64 {
        let mut count = 0;
        for section in &self.sections {
            count += section.index.len() as u64;
        }
        count
    }

    pub fn iter(&self) -> impl Iterator<Item = (RoaringBitmap, &[u32])> {
        self.sections.iter().flat_map(|x| {
            x.index.iter().map(|&i| {
                let bitmap = RoaringBitmap::deserialize_from(Cursor::new(&x.data[i..]))
                    .expect("Failed to parse RoaringBitmap!");
                (bitmap, x.codepoint_list.as_slice())
            })
        })
    }
}

/// Serialization code
impl BitsetList {
    pub fn serialize(&self, name: &str, data: &mut DataPackageEncoder) -> Result<()> {
        data.insert_data(
            &format!("{name}:bitset_list"),
            bincode::encode_to_vec(&self, config::standard())?,
        );
        Ok(())
    }

    pub fn deserialize(name: &str, data: &DataPackage) -> Result<BitsetList> {
        let bitset_list = bincode::decode_from_slice::<Self, _>(
            data.get_data(&format!("{name}:bitset_list"))?,
            config::standard(),
        )?;
        Ok(bitset_list.0)
    }

    pub fn transfer(
        name: &str,
        data: &DataPackage,
        encoder: &mut DataPackageEncoder,
    ) -> Result<()> {
        let bitset_name = format!("{name}:bitset_list");
        encoder.insert_data(&bitset_name, data.get_data(&bitset_name)?.to_vec());
        Ok(())
    }
}
