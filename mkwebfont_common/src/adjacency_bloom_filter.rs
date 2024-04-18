use crate::{
    data_package::{DataPackage, DataPackageEncoder},
    wyhash::{wyhash, WyHashBuilder},
};
use anyhow::Result;
use bincode::{config, Decode, Encode};
use std::{
    collections::HashMap,
    sync::atomic::{AtomicU64, Ordering},
};
use tracing::log::debug;
use wyrand::WyRand;

const XXH_SEED_BASE: u64 = 0x463dd552aaeaa6a5;

#[derive(Copy, Clone, Decode, Encode, Debug)]
struct FilterParams {
    size: usize,
    mask: usize,
    hash_count: usize,
    seed_a: u64,
    seed_b: u64,
}
impl FilterParams {
    fn new(name: &str, size: usize, hash_count: usize) -> Self {
        assert!(size.is_power_of_two());
        let mask = size - 1;
        let mut hash = WyRand::new(wyhash(XXH_SEED_BASE, name));
        let seed_a = hash.rand();
        let seed_b = hash.rand();
        FilterParams { size, mask, hash_count, seed_a, seed_b }
    }

    fn validate(&self) {
        assert_ne!(self.size, 0);
        assert!(self.size.is_power_of_two());
        assert_eq!(self.mask, self.size - 1);
    }

    /// This takes self to avoid problems with borrowing. It's `Copy` anyways.
    fn do_hash(self, value: (u32, u32), mut func: impl FnMut(usize)) {
        let mut hash_a = wyhash(self.seed_a, &value);
        let mut hash_b = wyhash(self.seed_b, &value);

        for i in 0..self.hash_count {
            hash_b = hash_b.wrapping_add(i as u64);
            func(hash_a as usize & self.mask);
            hash_a = hash_a.wrapping_add(hash_b);
        }
    }
}

#[derive(Copy, Clone, Decode, Encode, Debug)]
struct ByteEncoder {
    log_min_value: f64,
    log_max_value: f64,
    exponent: f64,
    median: u64,
}
impl ByteEncoder {
    fn init_for_min_max(exponent: f64, min: u64, max: u64, median: u64) -> Self {
        let min = min.max(1) as f64;
        let max = max.max(1) as f64;
        ByteEncoder {
            log_min_value: min.log(exponent),
            log_max_value: max.log(exponent),
            exponent,
            median,
        }
    }

    pub fn encode(&self, value: u64) -> u8 {
        let value = (value as f64).log(self.exponent);
        let value = value.min(self.log_max_value).max(self.log_min_value);
        let value = (value - self.log_min_value) / (self.log_max_value - self.log_min_value);
        (value * (u8::MAX as f64)).round() as u8
    }

    pub fn decode(&self, value: u8) -> u64 {
        let value = value as f64 / (u8::MAX as f64);
        let value = value * (self.log_max_value - self.log_min_value) + self.log_min_value;
        let value = self.exponent.powf(value);
        value.round() as u64
    }
}

#[derive(Copy, Clone, Decode, Encode, Debug)]
pub struct GlyphInfo {
    pub count: u64,
    pub edge_total: f64,
    pub block_id: u32,
}

#[derive(Clone, Decode, Encode, Debug)]
struct Meta {
    name: String,
    params: FilterParams,
    edge_total: f64,
    edge_count: f64,
    glyphs: HashMap<u32, GlyphInfo, WyHashBuilder>,
}

pub struct BloomFilterBuilder {
    meta: Meta,
    exponent: f64,
    data: Vec<AtomicU64>,
    median_threshold: f64,
}
impl BloomFilterBuilder {
    pub fn new(
        name: &str,
        size: usize,
        hash_count: usize,
        glyph_info: HashMap<u32, GlyphInfo>,
        exponent: f64,
        edge_total: f64,
        edge_count: f64,
        median_threshold: f64,
    ) -> Self {
        let mut data = Vec::with_capacity(size);
        for _ in 0..size {
            data.push(AtomicU64::new(0));
        }

        BloomFilterBuilder {
            meta: Meta {
                name: name.to_string(),
                params: FilterParams::new(name, size, hash_count),
                edge_total,
                edge_count,
                glyphs: glyph_info.into_iter().collect(),
            },
            exponent,
            data,
            median_threshold,
        }
    }

    pub fn insert_pairing(&self, a: u32, b: u32, count: u64) {
        let value = (a.min(b), a.max(b));
        self.meta.params.do_hash(value, |i| {
            self.data[i].fetch_max(count, Ordering::Relaxed);
        });
    }

    pub fn finish(&self) -> AdjacencyBloomFilter {
        let median = {
            let mut median_tmp: Vec<_> = self
                .data
                .as_slice()
                .iter()
                .map(|x| x.load(Ordering::Relaxed))
                .collect();
            let target = (median_tmp.len() as f64 * self.median_threshold) as usize;
            *median_tmp.select_nth_unstable(target).1
        };

        let mut min = u64::MAX;
        let mut max = u64::MIN;
        for v in self.data.as_slice() {
            min = min.min(v.load(Ordering::Relaxed));
            max = max.max(v.load(Ordering::Relaxed));
        }
        debug!("Raw min/max: {min}-{max}");

        if min < median {
            min = median;
        }
        debug!("Filtered min/max: {min}-{max}");

        let encoder = ByteEncoder::init_for_min_max(self.exponent, min, max, median);
        debug!("Encoder: {encoder:?}");

        let mut bloom = AdjacencyBloomFilter::new(self.meta.clone(), encoder);
        for (i, v) in self.data.as_slice().iter().enumerate() {
            bloom.data[i] = encoder.encode(v.load(Ordering::Relaxed));
        }
        bloom
    }
}

pub struct AdjacencyBloomFilter {
    meta: Meta,
    encoder: ByteEncoder,
    glyphs: Vec<char>,
    data: Vec<u8>,
}
impl AdjacencyBloomFilter {
    fn new(meta: Meta, encoder: ByteEncoder) -> Self {
        meta.params.validate();

        let mut glyphs = Vec::new();
        for (ch, _) in &meta.glyphs {
            glyphs.push(char::from_u32(*ch).unwrap());
        }
        glyphs.sort();

        let size = meta.params.size;
        AdjacencyBloomFilter { meta, encoder, glyphs, data: vec![0; size] }
    }

    pub fn glyph_list(&self) -> &[char] {
        &self.glyphs
    }

    pub fn get_character_frequency(&self, ch: u32) -> u64 {
        if let Some(x) = self.meta.glyphs.get(&ch) {
            x.count
        } else {
            0
        }
    }

    fn get_block_id(&self, ch: u32) -> u32 {
        if let Some(x) = self.meta.glyphs.get(&ch) {
            x.block_id
        } else {
            0
        }
    }

    #[inline(never)]
    pub fn get_pairing(&self, a: u32, b: u32) -> u64 {
        if a != b {
            let value = (a.min(b), a.max(b));
            let mut min = u8::MAX;
            // SAFETY: `FilterParams` is validated in `AdjacencyBloomFilter::new`
            self.meta
                .params
                .do_hash(value, |i| min = min.min(unsafe { *self.data.get_unchecked(i) }));

            let value = self.encoder.decode(min);
            if value > self.encoder.median {
                value
            } else if self.get_block_id(a) == self.get_block_id(b) {
                self.encoder.median
            } else {
                0
            }
        } else {
            self.get_character_frequency(a)
        }
    }

    pub fn serialize(&self, data: &mut DataPackageEncoder, name: &str) -> Result<()> {
        data.insert_data(
            &format!("{name}:bloom_meta"),
            bincode::encode_to_vec(&self.meta, config::standard())?,
        );
        data.insert_data(
            &format!("{name}:bloom_encoder"),
            bincode::encode_to_vec(&self.encoder, config::standard())?,
        );
        data.insert_data(&format!("{name}:bloom_table"), self.data.as_slice().to_vec());
        Ok(())
    }

    pub fn deserialize(data: &DataPackage, name: &str) -> Result<AdjacencyBloomFilter> {
        let meta = bincode::decode_from_slice::<Meta, _>(
            data.get_data(&format!("{name}:bloom_meta"))?,
            config::standard(),
        )?;
        let encoder = bincode::decode_from_slice::<ByteEncoder, _>(
            data.get_data(&format!("{name}:bloom_encoder"))?,
            config::standard(),
        )?;

        let mut bloom = AdjacencyBloomFilter::new(meta.0, encoder.0);
        let table = data.get_data(&format!("{name}:bloom_table"))?;
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
                let ea = self.meta.glyphs.get(&(chars[i] as u32)).unwrap().edge_total;
                let eb = self.meta.glyphs.get(&(chars[j] as u32)).unwrap().edge_total;
                total += (ea * eb) / (2.0 * self.meta.edge_total);
            }
        }
        total
    }
    pub fn modularity(&self, chars: &[char]) -> f64 {
        (self.modularity_actual(chars) - self.modularity_expectation(chars))
            / (2.0 * self.meta.edge_total)
    }

    /// Very slow!!! For testing only.
    pub fn str_modularity(&self, s: &str) -> f64 {
        let s: Vec<_> = s.chars().collect();
        self.modularity(&s)
    }
}
