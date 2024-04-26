use crate::{
    data_package::{DataPackage, DataPackageEncoder},
    wyhash::WyHashBuilder,
};
use anyhow::Result;
use bincode::{config, Decode, Encode};
use roaring::RoaringBitmap;
use std::{
    collections::HashMap,
    sync::atomic::{AtomicU32, Ordering},
};
use tracing::{debug, info};

const PLACE_SENTINEL: u32 = u32::MAX;

fn triangle(n: usize) -> usize {
    n.checked_mul(n.checked_add(1).unwrap())
        .unwrap()
        .checked_div(2)
        .unwrap()
}

fn triangle_unchecked(n: usize) -> usize {
    (n * (n + 1)) / 2
}

fn place_idx_unchecked(place_a: usize, place_b: usize) -> usize {
    triangle_unchecked(place_a + 1) - (place_b + 1)
}

fn place_idx(place_a: usize, place_b: usize) -> usize {
    if place_a < place_b {
        place_idx(place_b, place_a)
    } else {
        triangle_unchecked(place_a + 1) - (place_b + 1)
    }
}

#[derive(Copy, Clone, Decode, Encode, Debug)]
struct ByteEncoder {
    resolution: f64,
    log_min_value: f64,
    log_max_value: f64,
    exponent: f64,
}
impl ByteEncoder {
    fn init_for_min_max(resolution: u8, exponent: f64, min: u64, max: u64) -> Self {
        assert!(resolution <= 254);
        let min = min.max(1) as f64;
        let max = max.max(1) as f64;
        ByteEncoder {
            resolution: resolution as f64,
            log_min_value: min.log(exponent),
            log_max_value: max.log(exponent),
            exponent,
        }
    }

    pub fn encode(&self, value: u64) -> u8 {
        if value == 0 {
            0
        } else {
            let value = (value as f64).log(self.exponent);
            let value = value.min(self.log_max_value).max(self.log_min_value);
            let value = (value - self.log_min_value) / (self.log_max_value - self.log_min_value);
            1 + (value * self.resolution).round() as u8
        }
    }

    pub fn decode(&self, value: u8) -> u64 {
        if value == 0 {
            0
        } else {
            let value = (value - 1) as f64 / self.resolution;
            let value = value * (self.log_max_value - self.log_min_value) + self.log_min_value;
            let value = self.exponent.powf(value);
            value.round() as u64
        }
    }
}

#[derive(Default)]
struct BlockIdAssigner {
    block_ids: HashMap<&'static str, Option<usize>, WyHashBuilder>,
}
impl BlockIdAssigner {
    fn assign_id(&mut self, block_name: Option<&'static str>) -> Option<usize> {
        if let Some(name) = block_name {
            if let Some(cached) = self.block_ids.get(name) {
                *cached
            } else {
                let real_name = match name {
                    x if x.contains("Latin Extended") => "Latin Extended",
                    x if x.contains("Ideographs Extension") => "CJK Extension",
                    x if x.contains("CJK Compatibility") => "CJK Compatibility",
                    x if x.contains("Private Use Area") => "PUA",
                    x => x,
                };
                if real_name != name {
                    self.assign_id(Some(real_name))
                } else {
                    let id = self.block_ids.len();
                    self.block_ids.insert(name, Some(id));
                    Some(id)
                }
            }
        } else {
            None
        }
    }
}

pub struct AdjacencyArrayBuilder {
    name: String,
    codepoint_list: Vec<u32>,
    places: HashMap<u32, usize, WyHashBuilder>,
    data: Vec<AtomicU32>,
}
impl AdjacencyArrayBuilder {
    pub fn new(name: &str, glyphs: &RoaringBitmap) -> Self {
        let mut codepoint_list = Vec::new();
        let mut places = HashMap::default();
        for glyph in glyphs {
            codepoint_list.push(glyph);
            places.insert(glyph, places.len());
        }

        let triangle_ct = triangle(places.len());
        info!(
            "Allocating {:.2} GiB for uncompressed adjacency map...",
            (4 * triangle_ct) as f64 / (1 << 30) as f64,
        );
        let mut data = Vec::with_capacity(triangle_ct);
        for _ in 0..triangle_ct {
            data.push(AtomicU32::new(0));
        }
        debug!("Allocation done...");

        AdjacencyArrayBuilder { name: name.to_string(), codepoint_list, places, data }
    }

    pub fn push_vector(&self, bitmap: &RoaringBitmap, tmp: &mut Vec<usize>) {
        tmp.clear();
        for glyph in bitmap {
            if let Some(glyph) = self.places.get(&glyph) {
                tmp.push(*glyph);
            }
        }

        for (i, place_a) in tmp.iter().enumerate() {
            for place_b in tmp.iter().skip(i) {
                self.data[place_idx_unchecked(*place_b, *place_a)].fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub fn build(
        &self,
        resolution: usize,
        exponent: f64,
        get_block_id: impl Fn(char) -> Option<&'static str>,
    ) -> AdjacencyArray {
        // Find maximum, minimum.
        debug!("Adjacency array metadata: min, max");
        let mut min = u32::MAX;
        let mut max = u32::MIN;
        for bit in &self.data {
            let val = bit.load(Ordering::Relaxed);
            if val != 0 {
                min = min.min(val);
                max = max.max(val);
            }
        }

        // Find edge count.
        debug!("Adjacency array metadata: edge_total");
        let mut count: f64 = 0.0;
        for i in 0..self.codepoint_list.len() {
            count += self.data[place_idx(i, i)].load(Ordering::Relaxed) as f64;
        }

        // Initialize hashmap with all characters.
        debug!("Adjacency array metadata: chars (blocks)");
        let mut char_data = HashMap::default();
        {
            let mut assigner = BlockIdAssigner::default();
            for i in 0..=0x10FFFF {
                if let Some(char) = char::from_u32(i) {
                    if let Some(block_id) = assigner.assign_id(get_block_id(char)) {
                        char_data.insert(i, CodepointInfo {
                            edge_total: 0.0,
                            block_id: block_id as u32,
                            place: PLACE_SENTINEL,
                        });
                    }
                }
            }
        }

        // Initialize place data.
        debug!("Adjacency array metadata: chars (place)");
        for (k, &v) in &self.places {
            char_data.get_mut(k).unwrap().place = v as u32;
        }

        // Finding edge maximum data.
        debug!("Adjacency array metadata: chars (edge_total)");
        for a in 0..self.codepoint_list.len() {
            let mut edge_total = 0.0;
            for b in 0..self.codepoint_list.len() {
                if a != b {
                    edge_total += self.data[place_idx(a, b)].load(Ordering::Relaxed) as f64;
                }
            }
            char_data
                .get_mut(&self.codepoint_list[a])
                .unwrap()
                .edge_total = edge_total;
        }

        // Encode final data vector
        debug!("Adjacency array metadata: encoder, final_data");
        let encoder =
            ByteEncoder::init_for_min_max(resolution as u8, exponent, min as u64, max as u64);
        debug!("Encoder: {encoder:?}");
        let mut final_data = Vec::with_capacity(self.data.len());
        for val in &self.data {
            final_data.push(encoder.encode(val.load(Ordering::Relaxed) as u64));
        }

        // Build final map
        let meta = Meta {
            name: self.name.clone(),
            encoder,
            codepoints: char_data,
            codepoint_list: self
                .codepoint_list
                .iter()
                .map(|x| char::from_u32(*x).unwrap())
                .collect(),
            edge_total: count,
        };
        AdjacencyArray { meta, data: final_data }
    }
}

#[derive(Copy, Clone, Decode, Encode, Debug)]
struct CodepointInfo {
    edge_total: f64,
    block_id: u32,
    place: u32, // could be usize, but, alignment
}
impl CodepointInfo {
    fn has_place(&self) -> bool {
        self.place != PLACE_SENTINEL
    }
    fn place(&self) -> usize {
        self.place as usize
    }
}

#[derive(Clone, Decode, Encode)]
struct Meta {
    name: String,
    encoder: ByteEncoder,
    codepoints: HashMap<u32, CodepointInfo, WyHashBuilder>,
    codepoint_list: Vec<char>,
    edge_total: f64,
}

#[derive(Clone, Decode, Encode)]
pub struct AdjacencyArray {
    meta: Meta,
    data: Vec<u8>,
}
impl AdjacencyArray {
    pub fn glyph_list(&self) -> &[char] {
        &self.meta.codepoint_list
    }

    pub fn get_character_frequency(&self, ch: u32) -> u64 {
        if let Some(data_ch) = self.meta.codepoints.get(&ch) {
            if data_ch.has_place() {
                let place = data_ch.place();
                self.meta.encoder.decode(self.data[place_idx(place, place)])
            } else {
                0
            }
        } else {
            0
        }
    }

    pub fn is_same_block(&self, a: u32, b: u32) -> bool {
        if a != b {
            let Some(data_a) = self.meta.codepoints.get(&a) else {
                return false;
            };
            let Some(data_b) = self.meta.codepoints.get(&b) else {
                return false;
            };
            data_a.block_id == data_b.block_id
        } else {
            true
        }
    }

    pub fn get_pairing(&self, a: u32, b: u32) -> u64 {
        if a != b {
            let Some(data_a) = self.meta.codepoints.get(&a) else {
                return 0;
            };
            let Some(data_b) = self.meta.codepoints.get(&b) else {
                return 0;
            };
            if !data_a.has_place() || !data_b.has_place() {
                return 0;
            }

            self.meta
                .encoder
                .decode(self.data[place_idx(data_a.place(), data_b.place())])
        } else {
            self.get_character_frequency(a)
        }
    }

    pub fn serialize(&self, name: &str, data: &mut DataPackageEncoder) -> Result<()> {
        data.insert_data(
            &format!("{name}:adjacency_array_meta"),
            bincode::encode_to_vec(&self.meta, config::standard())?,
        );
        data.insert_data(&format!("{name}:adjacency_array"), self.data.clone());
        Ok(())
    }

    pub fn deserialize(name: &str, data: &DataPackage) -> Result<AdjacencyArray> {
        let meta = bincode::decode_from_slice::<Meta, _>(
            data.get_data(&format!("{name}:adjacency_array_meta"))?,
            config::standard(),
        )?;
        let data = data.get_data(&format!("{name}:adjacency_array"))?;
        Ok(AdjacencyArray { meta: meta.0, data: data.to_vec() })
    }

    pub fn transfer(
        name: &str,
        data: &DataPackage,
        encoder: &mut DataPackageEncoder,
    ) -> Result<()> {
        let meta_name = format!("{name}:adjacency_array_meta");
        let array_name = format!("{name}:adjacency_array");
        encoder.insert_data(&meta_name, data.get_data(&meta_name)?.to_vec());
        encoder.insert_data(&array_name, data.get_data(&array_name)?.to_vec());
        Ok(())
    }

    /// Returns the change in modularity if a character would be added to a set of characters.
    pub fn delta_modularity(&self, target: char, set: &[char]) -> f64 {
        let mut total = 0.0;

        // calculate modularity expectation
        let ea = self
            .meta
            .codepoints
            .get(&(target as u32))
            .unwrap()
            .edge_total;
        for char in set {
            let eb = self
                .meta
                .codepoints
                .get(&(*char as u32))
                .unwrap()
                .edge_total;
            total -= eb;
        }
        total *= ea / (2.0 * self.meta.edge_total);

        // calculate actual modularity
        for char in set {
            total += self.get_pairing(target as u32, *char as u32) as f64;
        }
        total
    }
}