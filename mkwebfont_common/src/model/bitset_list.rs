use crate::{
    model::data_package::{DataPackage, DataPackageEncoder},
    wyhash::WyHashBuilder,
};
use anyhow::Result;
use bincode::{config, Decode, Encode};
use roaring::RoaringBitmap;
use std::{
    collections::HashMap,
    io::{Cursor, Seek, SeekFrom},
    sync::Arc,
};
use tracing::debug;

pub struct BitsetListBuilder {
    source: String,
    codepoint_list: Vec<u32>,
    glyph_mapping: HashMap<u32, u32, WyHashBuilder>,
    index: Vec<usize>,
    data: Vec<u8>,

    used_cd_idx: u16,
    used_cd: Box<[u16; 0x110000]>,
    filtered: Box<[bool; 0x110000]>,
}
impl BitsetListBuilder {
    pub fn new(source: &str) -> Self {
        BitsetListBuilder {
            source: source.to_string(),
            codepoint_list: vec![],
            glyph_mapping: Default::default(),
            index: vec![],
            data: vec![],
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

    fn cursor(&mut self) -> Cursor<&mut Vec<u8>> {
        let mut cursor = Cursor::new(&mut self.data);
        cursor.seek(SeekFrom::End(0)).unwrap();
        cursor
    }

    pub fn push_sample(&mut self, str: &str) {
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
                } else {
                    continue;
                }

                if !self.filtered[ch as usize] {
                    bitset.insert(self.map_ch(ch as u32));
                }
            }
        }

        self.index.push(self.data.len());
        bitset.serialize_into(self.cursor()).unwrap();
    }

    fn push_sample_from_bitset(&mut self, list: &[u32], src: RoaringBitmap) {
        let mut bitset = RoaringBitmap::new();
        for ch in src {
            bitset.insert(self.map_ch(list[ch as usize]));
        }
        self.index.push(self.data.len());
        bitset.serialize_into(self.cursor()).unwrap();
    }

    pub fn mapping(&self) -> &[u32] {
        &self.codepoint_list
    }

    fn iter<'a>(&'a self) -> impl Iterator<Item = RoaringBitmap> + 'a {
        self.index
            .iter()
            .map(|x| RoaringBitmap::deserialize_from(Cursor::new(&self.data[*x..])).unwrap())
    }

    pub fn optimize(&self) -> BitsetListBuilder {
        debug!("Optimizing bitset section: {}", self.source);

        let mut optimzied = BitsetListBuilder::new(&self.source);

        // Calculate the frequency of each character
        let mut frequency = vec![0usize; self.codepoint_list.len()];
        for bitset in self.iter() {
            for bit in bitset {
                frequency[bit as usize] += 1;
            }
        }

        // Map characters in frequency order
        let mut frequency_sort = Vec::new();
        for (i, ch) in self.mapping().iter().enumerate() {
            frequency_sort.push((*ch, frequency[i]));
        }
        frequency_sort.sort_by_key(|x| x.1);
        frequency_sort.reverse();
        for (ch, _) in frequency_sort {
            optimzied.map_ch(ch);
        }

        // Reencode the bitsets
        for bitset in self.iter() {
            optimzied.push_sample_from_bitset(&self.codepoint_list, bitset);
        }

        debug!(
            "Optimized size: {} = {:.2} MiB",
            self.source,
            (self.data.len() as f64) / 1024.0 / 1024.0,
        );
        optimzied
    }
}

pub fn build(raw_sections: Vec<BitsetListBuilder>) -> BitsetList {
    let mut sections = Vec::new();
    for section in raw_sections {
        sections.push(Arc::new(BitsetSection {
            source: section.source,
            index: section.index,
            codepoint_list: section.codepoint_list,
            data: section.data,
        }));
    }
    BitsetList { sections }
}

#[derive(Clone, Decode, Encode)]
pub struct BitsetSection {
    source: String,
    index: Vec<usize>,
    codepoint_list: Vec<u32>,
    data: Vec<u8>,
}
impl BitsetSection {
    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn chars(&self) -> &[u32] {
        &self.codepoint_list
    }

    pub fn iter<'a>(&'a self) -> impl Iterator<Item = RoaringBitmap> + 'a {
        self.index
            .iter()
            .map(|x| RoaringBitmap::deserialize_from(Cursor::new(&self.data[*x..])).unwrap())
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
            count += section.index.len() as u64;
        }
        count
    }

    pub fn sections(&self) -> &[Arc<BitsetSection>] {
        &self.sections
    }

    pub fn take_sections(self) -> Vec<Arc<BitsetSection>> {
        self.sections
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
