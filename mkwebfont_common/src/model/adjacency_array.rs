use crate::{
    model::data_package::{DataPackage, DataPackageEncoder},
    wyhash::WyHashBuilder,
};
use anyhow::Result;
use bincode::{config, Decode, Encode};
use roaring::RoaringBitmap;
use std::collections::HashMap;
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

fn place_idx(place_a: usize, place_b: usize) -> usize {
    if place_a < place_b {
        place_idx(place_b, place_a)
    } else {
        triangle_unchecked(place_a + 1) - (place_b + 1)
    }
}

#[derive(Copy, Clone, Decode, Encode, Debug)]
struct ByteEncoder {
    exponent: f64,
}
impl ByteEncoder {
    fn new(exponent: f64) -> Self {
        ByteEncoder { exponent }
    }

    pub fn encode(&self, value: u64) -> u8 {
        if value == 0 {
            0
        } else {
            let value = (value as f64).log(self.exponent);
            1 + value.round() as u8
        }
    }

    pub fn decode(&self, value: u8) -> u64 {
        if value == 0 {
            0
        } else {
            let value = self.exponent.powf((value - 1) as f64);
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
    data: Vec<u32>,
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
            data.push(0);
        }
        debug!("Allocation done...");

        AdjacencyArrayBuilder { name: name.to_string(), codepoint_list, places, data }
    }

    pub fn push_vector(&mut self, bitmap: &RoaringBitmap, chars: &[u32], tmp: &mut Vec<usize>) {
        tmp.clear();
        for glyph in bitmap {
            if let Some(glyph) = self.places.get(&chars[glyph as usize]) {
                tmp.push(*glyph);
            }
        }

        for (i, place_a) in tmp.iter().enumerate() {
            for place_b in tmp.iter().skip(i) {
                self.data[place_idx(*place_b, *place_a)] += 1;
            }
        }
    }

    pub fn join(&mut self, other: AdjacencyArrayBuilder) {
        assert_eq!(&self.codepoint_list, &other.codepoint_list);
        assert_eq!(&self.places, &other.places);
        assert_eq!(self.data.len(), other.data.len());
        for i in 0..self.data.len() {
            self.data[i] += other.data[i];
        }
    }

    pub fn build(
        &self,
        exponent: f64,
        get_block_id: impl Fn(char) -> Option<&'static str>,
    ) -> AdjacencyArray {
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
        debug!("Adjacency array metadata: chars (edge_total), edge_total");
        let mut count: f64 = 0.0;
        for a in 0..self.codepoint_list.len() {
            let mut edge_total = 0.0;
            for b in 0..self.codepoint_list.len() {
                if a != b {
                    let value = self.data[place_idx(a, b)] as f64;
                    edge_total += value;
                    if a < b {
                        count += value;
                    }
                }
            }
            char_data
                .get_mut(&self.codepoint_list[a])
                .unwrap()
                .edge_total = edge_total;
        }

        // Encode final data vector
        debug!("Adjacency array metadata: encoder, final_data");
        let encoder = ByteEncoder::new(exponent);
        let mut final_data = Vec::with_capacity(self.data.len());
        for &val in &self.data {
            final_data.push(encoder.encode(val as u64));
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

            let is_same_block = (data_a.block_id == data_b.block_id) as u64;
            if !data_a.has_place() || !data_b.has_place() {
                return is_same_block;
            }
            let data = self.data[place_idx(data_a.place(), data_b.place())];
            self.meta.encoder.decode(data) + is_same_block
        } else {
            self.get_character_frequency(a)
        }
    }

    fn get_edge_total(&self, ch: char) -> f64 {
        self.meta.codepoints.get(&(ch as u32)).unwrap().edge_total
    }

    pub fn with_name(mut self, name: &str) -> Self {
        self.meta.name = name.to_string();
        self
    }

    /// Returns the modularity of a set of characters
    pub fn modularity(&self, set: &[char]) -> f64 {
        let mut modularity = 0.0;
        for a in 0..set.len() {
            for b in a + 1..set.len() {
                let ca = set[a];
                let cb = set[b];
                modularity += self.get_pairing(ca as u32, cb as u32) as f64;
                let ea = self.get_edge_total(ca).max(1.0);
                let eb = self.get_edge_total(cb).max(1.0);
                modularity -= (ea * eb) / (2.0 * self.meta.edge_total);
            }
        }
        modularity / (2.0 * self.meta.edge_total)
    }

    /// Returns the change in modularity if a character would be added to a set of characters.
    pub fn delta_modularity(&self, target: char, set: &[char]) -> f64 {
        let mut total = 0.0;

        // calculate modularity expectation
        let ea = self.get_edge_total(target);
        for &char in set {
            let eb = self.get_edge_total(char);
            total -= eb.max(1.0); // always expect at least one edge
        }
        total *= ea / (2.0 * self.meta.edge_total);

        // calculate actual modularity
        for char in set {
            total += self.get_pairing(target as u32, *char as u32) as f64;
        }
        total
    }
}

/// Serialization code
impl AdjacencyArray {
    pub fn serialize(&self, name: &str, data: &mut DataPackageEncoder) -> Result<()> {
        data.insert_data(
            &format!("{name}:adjacency_array_meta"),
            bincode::encode_to_vec(&self.meta, config::standard())?,
        );
        data.insert_data(&format!("{name}:adjacency_array"), self.data.clone());
        Ok(())
    }

    pub fn deserialize(name: &str, data: &mut DataPackage) -> Result<AdjacencyArray> {
        let meta: Meta = data.take_bincode(&format!("{name}:adjacency_array_meta"))?;
        let data = data.take_data(&format!("{name}:adjacency_array"))?;
        Ok(AdjacencyArray { meta, data })
    }
}