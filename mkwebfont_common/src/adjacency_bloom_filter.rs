use anyhow::Result;
use bincode::{Decode, Encode};
use std::{
    collections::HashMap,
    fs::File,
    hash::Hash,
    io::{BufWriter, Write},
    path::Path,
};
use xxhash_rust::xxh3::{Xxh3, Xxh3Builder};
use zstd::Encoder;

const FORMAT_VERSION: &str = "adjacency_format:v1";

const BLOOM_FILTER_SIZE: usize = (1 << 20) * 256; // 256 MiB
const BLOOM_FILTER_COUNT: usize = 5;
const XXH_SEED_A: u64 = 0xde66789c738d6e58;
const XXH_SEED_B: u64 = 0x99c33e4ae16946a0;

const SERIALIZE_SEGMENTS: usize = 8;
const SERIALIZE_SEGMENT_SIZE: usize = BLOOM_FILTER_SIZE / SERIALIZE_SEGMENTS;

#[derive(Decode, Encode, Debug)]
pub struct FilterInfo {
    pub log_min_value: f64,
    pub log_max_value: f64,
    pub exponent: f64,
    pub edge_total: f64,
    pub edge_count: f64,
}
impl FilterInfo {
    pub fn init_for_min_max(
        exponent: f64,
        min: u32,
        max: u32,
        edge_total: f64,
        edge_count: f64,
    ) -> Self {
        let min = min.max(1) as f64;
        let max = max.max(1) as f64;
        FilterInfo {
            log_min_value: min.log(exponent),
            log_max_value: max.log(exponent),
            exponent,
            edge_total,
            edge_count,
        }
    }

    pub fn encode(&self, value: u32) -> u8 {
        let value = (value as f64).log(self.exponent);
        let value = value.min(self.log_max_value).max(self.log_min_value);
        let value = value / (self.log_max_value - self.log_min_value) - self.log_min_value;
        (value * (u8::MAX as f64)).round() as u8
    }

    pub fn decode(&self, value: u8) -> u32 {
        let value = value as f64 / (u8::MAX as f64);
        let value = value * (self.log_max_value - self.log_min_value) + self.log_min_value;
        let value = value.powf(self.exponent);
        value.round() as u32
    }
}

fn xxh3_seed(seed: u64, data: impl Hash) -> u64 {
    let mut xxh = Xxh3::with_seed(seed);
    data.hash(&mut xxh);
    xxh.digest()
}

#[derive(Decode, Encode, Debug)]
pub struct GlyphInfo {
    pub count: u32,
    pub edge_total: f64,
}

#[derive(Decode, Encode, Debug)]
struct Meta {
    filter_info: FilterInfo,
    glyph_info: HashMap<char, GlyphInfo, Xxh3Builder>,
}

pub struct AdjacencyBloomFilter {
    meta: Meta,
    glyphs: Vec<char>,
    data: Vec<u8>,
}
impl AdjacencyBloomFilter {
    pub fn new(glyph_info: HashMap<char, GlyphInfo>, filter_info: FilterInfo) -> Self {
        let mut glyphs = Vec::new();
        for (ch, _) in &glyph_info {
            glyphs.push(*ch);
        }
        glyphs.sort();

        AdjacencyBloomFilter {
            meta: Meta { filter_info, glyph_info: glyph_info.into_iter().collect() },
            glyphs,
            data: vec![0; BLOOM_FILTER_SIZE],
        }
    }

    fn do_hash(value: (u32, u32), mut func: impl FnMut(usize)) {
        let mut hash_a = xxh3_seed(XXH_SEED_A, value);
        let mut hash_b = xxh3_seed(XXH_SEED_B, value);
        for i in 0..BLOOM_FILTER_COUNT {
            hash_b = hash_b.wrapping_add(i as u64);
            func((hash_a % BLOOM_FILTER_SIZE as u64) as usize);
            hash_a = hash_a.wrapping_add(hash_b);
        }
    }

    pub fn glyph_list(&self) -> &[char] {
        &self.glyphs
    }

    pub fn glyph_info(&self, glyph: char) -> Option<&GlyphInfo> {
        self.meta.glyph_info.get(&glyph)
    }

    pub fn insert_pairing(&mut self, a: u32, b: u32, count: u32) {
        let value = (a.min(b), a.max(b));
        let count = self.meta.filter_info.encode(count);
        Self::do_hash(value, |i| self.data[i] = self.data[i].max(count));
    }
    pub fn load_pairing(&mut self, a: u32, b: u32) -> u32 {
        let value = (a.min(b), a.max(b));
        let mut min = u8::MAX;
        Self::do_hash(value, |i| min = min.min(self.data[i]));
        self.meta.filter_info.decode(min)
    }

    pub fn serialize_to_dir(&self, target: impl AsRef<Path>) -> Result<()> {
        if target.as_ref().exists() {
            std::fs::remove_dir_all(&target)?;
        }
        std::fs::create_dir_all(&target)?;

        let mut path = target.as_ref().to_path_buf();
        for i in 0..SERIALIZE_SEGMENTS {
            let range = SERIALIZE_SEGMENT_SIZE * i..SERIALIZE_SEGMENT_SIZE * (i + 1);

            path.push(format!("adjacency_section_{i}.zst"));
            let mut zstd = Encoder::new(BufWriter::new(File::create(&path)?), 20)?;
            zstd.write_all(&self.data[range])?;
            zstd.finish()?;
            path.pop();
        }

        path.push("adjacency_meta.zst");
        {
            let mut zstd = Encoder::new(BufWriter::new(File::create(&path)?), 20)?;
            let data = bincode::encode_to_vec(&self.meta, bincode::config::standard())?;
            zstd.write_all(&data)?;
            zstd.finish()?;
        };
        path.pop();

        path.push("format_version");
        std::fs::write(&path, FORMAT_VERSION)?;

        Ok(())
    }
}
