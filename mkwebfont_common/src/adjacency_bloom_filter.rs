use crate::{
    data_package::{DataPackage, DataPackageEncoder},
    wyhash::{wyhash, WyHashBuilder},
};
use anyhow::Result;
use bincode::{config, Decode, Encode};
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::atomic::{AtomicU32, Ordering},
};
use tracing::log::debug;

const BLOOM_FILTER_SIZE: usize = (1 << 20) * 256; // 256 MiB
const BLOOM_FILTER_COUNT: usize = 6;
const XXH_SEED_A: u64 = 0xde66789c738d6e58;
const XXH_SEED_B: u64 = 0x99c33e4ae16946a0;

pub struct FilterParams {
    size: usize,
    mask: usize,
    hash_count: usize,
    seed_a: u64,
    seed_b: u64,
}
impl FilterParams {
    pub fn new(size: usize, hash_count: usize, seed_a: u64, seed_b: u64) -> Self {
        assert!(size.is_power_of_two());
        let mask = size - 1;
        FilterParams { size, mask, hash_count, seed_a, seed_b }
    }
}

#[derive(Copy, Clone, Decode, Encode, Debug)]
struct FilterInfo {
    pub log_min_value: f64,
    pub log_max_value: f64,
    pub exponent: f64,
    pub edge_total: f64,
    pub edge_count: f64,
    pub median: u32,
}
impl FilterInfo {
    fn init_for_min_max(
        exponent: f64,
        min: u32,
        max: u32,
        edge_total: f64,
        edge_count: f64,
        median: u32,
    ) -> Self {
        let min = min.max(1) as f64;
        let max = max.max(1) as f64;
        FilterInfo {
            log_min_value: min.log(exponent),
            log_max_value: max.log(exponent),
            exponent,
            edge_total,
            edge_count,
            median,
        }
    }

    pub fn encode(&self, value: u32) -> u8 {
        let value = (value as f64).log(self.exponent);
        let value = value.min(self.log_max_value).max(self.log_min_value);
        let value = (value - self.log_min_value) / (self.log_max_value - self.log_min_value);
        (value * (u8::MAX as f64)).round() as u8
    }

    pub fn decode(&self, value: u8) -> u32 {
        let value = value as f64 / (u8::MAX as f64);
        let value = value * (self.log_max_value - self.log_min_value) + self.log_min_value;
        let value = self.exponent.powf(value);
        value.round() as u32
    }
}

#[derive(Copy, Clone, Decode, Encode, Debug)]
pub struct GlyphInfo {
    pub count: u32,
    pub edge_total: f64,
}

#[derive(Clone, Decode, Encode, Debug)]
struct Meta {
    filter_info: FilterInfo,
    glyph_info: HashMap<char, GlyphInfo, WyHashBuilder>,
}

fn do_hash(value: (u32, u32), mut func: impl FnMut(usize)) {
    let mut hash_a = wyhash(XXH_SEED_A, &value);
    let mut hash_b = wyhash(XXH_SEED_B, &value);

    for i in 0..BLOOM_FILTER_COUNT {
        hash_b = hash_b.wrapping_add(i as u64);
        func((hash_a % BLOOM_FILTER_SIZE as u64) as usize);
        hash_a = hash_a.wrapping_add(hash_b);
    }
}

pub struct BloomFilterBuilder {
    exponent: f64,
    edge_total: f64,
    edge_count: f64,
    glyphs: HashMap<char, GlyphInfo, WyHashBuilder>,
    data: Box<[AtomicU32; BLOOM_FILTER_SIZE]>,
}
impl BloomFilterBuilder {
    pub fn new(
        glyph_info: HashMap<char, GlyphInfo>,
        exponent: f64,
        edge_total: f64,
        edge_count: f64,
    ) -> Self {
        BloomFilterBuilder {
            exponent,
            edge_total,
            edge_count,
            glyphs: glyph_info.into_iter().collect(),
            data: unsafe { Box::new_zeroed().assume_init() },
        }
    }

    pub fn insert_pairing(&self, a: u32, b: u32, count: u32) {
        let value = (a.min(b), a.max(b));
        do_hash(value, |i| {
            self.data[i].fetch_max(count, Ordering::Relaxed);
        });
    }

    pub fn finish(&self) -> AdjacencyBloomFilter {
        let mut min = u32::MAX;
        let mut max = u32::MIN;
        for v in self.data.as_slice() {
            min = min.min(v.load(Ordering::Relaxed));
            max = max.max(v.load(Ordering::Relaxed));
        }
        debug!("Raw min/max: {min}-{max}");

        let median = {
            let mut median_tmp: Vec<_> = self
                .data
                .as_slice()
                .iter()
                .map(|x| x.load(Ordering::Relaxed))
                .collect();
            let target = median_tmp.len() / 2;
            *median_tmp.select_nth_unstable(target).1
        };
        let filter_info = FilterInfo::init_for_min_max(
            self.exponent,
            min,
            max,
            self.edge_total,
            self.edge_count,
            median,
        );
        debug!("Filter info: {filter_info:?}");

        let mut bloom = AdjacencyBloomFilter::new(self.glyphs.clone(), filter_info);
        for (i, v) in self.data.as_slice().iter().enumerate() {
            bloom.data[i] = filter_info.encode(v.load(Ordering::Relaxed));
        }
        bloom
    }
}

pub struct AdjacencyBloomFilter {
    meta: Meta,
    glyphs: Vec<char>,
    data: Box<[u8; BLOOM_FILTER_SIZE]>,
}
impl AdjacencyBloomFilter {
    fn new(glyph_info: HashMap<char, GlyphInfo, WyHashBuilder>, filter_info: FilterInfo) -> Self {
        let mut glyphs = Vec::new();
        for (ch, _) in &glyph_info {
            glyphs.push(*ch);
        }
        glyphs.sort();

        AdjacencyBloomFilter {
            meta: Meta { filter_info, glyph_info: glyph_info.into_iter().collect() },
            glyphs,
            data: unsafe { Box::new_zeroed().assume_init() },
        }
    }

    pub fn glyph_list(&self) -> &[char] {
        &self.glyphs
    }

    pub fn glyph_info(&self, glyph: char) -> Option<&GlyphInfo> {
        self.meta.glyph_info.get(&glyph)
    }

    pub fn get_character_frequency(&self, ch: u32) -> u32 {
        if let Some(x) = self.meta.glyph_info.get(&char::from_u32(ch).unwrap()) {
            x.count
        } else {
            0
        }
    }

    pub fn get_pairing(&self, a: u32, b: u32) -> u32 {
        if a != b {
            let value = (a.min(b), a.max(b));
            let mut min = u8::MAX;
            do_hash(value, |i| min = min.min(self.data[i]));
            self.meta.filter_info.decode(min)
        } else {
            self.get_character_frequency(a)
        }
    }

    pub fn serialize(&self, data: &mut DataPackageEncoder) -> Result<()> {
        data.insert_data("adjacency_table", self.data.as_slice().to_vec());
        data.insert_data("adjacency_meta", bincode::encode_to_vec(&self.meta, config::standard())?);
        Ok(())
    }

    pub fn deserialize(data: &DataPackage) -> Result<AdjacencyBloomFilter> {
        let meta = bincode::decode_from_slice::<Meta, _>(
            data.get_data("adjacency_meta")?,
            config::standard(),
        )?;
        let mut bloom = AdjacencyBloomFilter::new(meta.0.glyph_info, meta.0.filter_info);
        let table = data.get_data("adjacency_table")?;
        bloom.data.copy_from_slice(table);
        Ok(bloom)
    }

    fn modularity_actual(&self, chars: &[char]) -> f64 {
        let mut total = 0.0;
        for i in 0..chars.len() {
            for j in i + 1..chars.len() {
                total += self.get_pairing(chars[i] as u32, chars[j] as u32) as f64;
            }
        }
        total
    }
    fn modularity_expectation(&self, chars: &[char]) -> f64 {
        let mut total = 0.0;
        for i in 0..chars.len() {
            for j in i + 1..chars.len() {
                let ea = self.meta.glyph_info.get(&chars[i]).unwrap().edge_total;
                let eb = self.meta.glyph_info.get(&chars[j]).unwrap().edge_total;
                total += (ea * eb) / (2.0 * self.meta.filter_info.edge_total);
            }
        }
        total
    }
    pub fn modularity(&self, chars: &[char]) -> f64 {
        (self.modularity_actual(chars) - self.modularity_expectation(chars))
            / (2.0 * self.meta.filter_info.edge_total)
    }

    /// Very slow!!! For testing only.
    pub fn str_modularity(&self, s: &str) -> f64 {
        let s: Vec<_> = s.chars().collect();
        self.modularity(&s)
    }
}
