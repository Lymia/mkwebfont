use anyhow::Result;
use roaring::RoaringBitmap;
use std::{
    collections::HashMap,
    fs::File,
    io::{BufReader, BufWriter, Write},
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
};
use tracing::{debug, info};
use unic_ucd_category::GeneralCategory;
use zstd::{Decoder, Encoder};

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

pub struct AdjacencyInfo {
    codepoint_list: Vec<u32>,
    places: HashMap<u32, usize>,
    data: Vec<AtomicU32>,
}
impl AdjacencyInfo {
    fn new(glyphs: RoaringBitmap) -> Self {
        let mut codepoint_list = Vec::new();
        let mut places = HashMap::new();
        for glyph in glyphs {
            codepoint_list.push(glyph);
            places.insert(glyph, places.len());
        }

        let triangle_ct = triangle(places.len());
        info!(
            "Allocating {:.2} GiB for adjacency information...",
            (4 * triangle_ct) as f64 / (1 << 30) as f64,
        );
        let mut data = Vec::with_capacity(triangle_ct);
        for _ in 0..triangle_ct {
            data.push(AtomicU32::new(0));
        }
        debug!("Allocation done...");

        AdjacencyInfo { codepoint_list, places, data }
    }

    fn push_vector(&self, bitmap: &RoaringBitmap, tmp: &mut Vec<usize>) {
        for glyph in bitmap {
            if let Some(glyph) = self.places.get(&glyph) {
                tmp.push(*glyph);
            }
        }

        for (i, place_a) in tmp.iter().enumerate() {
            for place_b in tmp.iter().skip(i) {
                self.data[place_idx(*place_a, *place_b)].fetch_add(1, Ordering::Relaxed);
            }
        }
        tmp.clear();
    }

    pub fn get_codepoint_count(&self, ch: char) -> u32 {
        self.get_cooccurance_count(ch, ch)
    }

    pub fn get_cooccurance_count(&self, cha: char, chb: char) -> u32 {
        if let Some(cha) = self.places.get(&(cha as u32)) {
            if let Some(chb) = self.places.get(&(chb as u32)) {
                return self.data[place_idx(*cha, *chb)].load(Ordering::Relaxed);
            }
        }
        0
    }

    fn serialize(&self, into: &mut impl Write) -> Result<()> {
        into.write_all(&self.codepoint_list.len().to_le_bytes())?;
        for codepoint in &self.codepoint_list {
            into.write_all(&codepoint.to_le_bytes())?;
        }

        into.write_all(&self.data.len().to_le_bytes())?;
        for data in &self.data {
            into.write_all(&data.load(Ordering::Relaxed).to_le_bytes())?;
        }

        Ok(())
    }
}

async fn push_to_table(
    i: usize,
    webpage_count: u64,
    adjancency: Arc<AdjacencyInfo>,
    bitmaps: Vec<RoaringBitmap>,
) {
    info!("Processing {} pages as of {i}/{webpage_count} bitmaps ...", bitmaps.len());
    let mut tmp = Vec::new();
    for bitmap in bitmaps {
        adjancency.push_vector(&bitmap, &mut tmp);
    }
}

pub async fn generate_adjacency_table() -> Result<()> {
    let mut all_glyphs = RoaringBitmap::new();
    let mut webpage_count = 0u64;
    {
        let path = File::open("run/common-crawl_parsed-bitmaps.zst")?;
        let reader = BufReader::new(path);
        let mut zstd = Decoder::new(reader)?;

        while let Ok(bitmap) = RoaringBitmap::deserialize_from(&mut zstd) {
            for ch in bitmap {
                all_glyphs.insert(ch);
            }
            webpage_count += 1;
        }
    }

    let mut filtered_glyphs = RoaringBitmap::new();
    for glyph in &all_glyphs {
        let ch = char::from_u32(glyph).unwrap();
        let cat = GeneralCategory::of(ch);
        if !cat.is_other() && !cat.is_separator() {
            filtered_glyphs.insert(glyph);
        }
    }

    info!("Codepoint count: {}", all_glyphs.len());
    info!("Webpage count: {webpage_count}");
    info!("Filtered codepoint count: {}", filtered_glyphs.len());

    let graph = Arc::new(AdjacencyInfo::new(filtered_glyphs.clone()));
    {
        let path = File::open("run/common-crawl_parsed-bitmaps.zst")?;
        let reader = BufReader::new(path);
        let mut zstd = Decoder::new(reader)?;

        let mut i = 0;
        let mut bitmaps = Vec::new();
        let mut threads = Vec::new();
        while let Ok(bitmap) = RoaringBitmap::deserialize_unchecked_from(&mut zstd) {
            bitmaps.push(bitmap);

            let graph = graph.clone();
            let task = tokio::spawn(push_to_table(
                i,
                webpage_count,
                graph,
                std::mem::replace(&mut bitmaps, Vec::new()),
            ));
            threads.push(task);

            i += 1;
            if i % 200000 == 0 {
                debug!("Submitting bitmaps as of {i}/{webpage_count} for processing...");
            }
        }

        for thread in threads {
            thread.await?;
        }
    }

    {
        let path = File::create("run/common-crawl_adjacency.zst")?;
        let writer = BufWriter::new(path);
        let mut zstd = Encoder::new(writer, 10)?;
        graph.serialize(&mut zstd)?;
        zstd.finish()?;
    }

    Ok(())
}
