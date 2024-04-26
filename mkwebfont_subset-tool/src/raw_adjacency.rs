use anyhow::Result;
use roaring::RoaringBitmap;
use std::{
    collections::HashMap,
    fs::File,
    io::{BufReader, BufWriter, Read, Write},
    path::Path,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
};
use tracing::{debug, info};
use unic_ucd_category::GeneralCategory;
use zstd::{Decoder, Encoder};

pub fn triangle(n: usize) -> usize {
    n.checked_mul(n.checked_add(1).unwrap())
        .unwrap()
        .checked_div(2)
        .unwrap()
}

pub fn triangle_unchecked(n: usize) -> usize {
    (n * (n + 1)) / 2
}

pub fn place_idx(place_a: usize, place_b: usize) -> usize {
    if place_a < place_b {
        place_idx(place_b, place_a)
    } else {
        triangle_unchecked(place_a + 1) - (place_b + 1)
    }
}

async fn push_to_table(
    i: usize,
    webpage_count: u64,
    adjacency: Arc<RawAdjacencyInfo>,
    bitmaps: Vec<RoaringBitmap>,
) {
    info!("Processing {} pages as of {i}/{webpage_count} bitmaps ...", bitmaps.len());
    let mut tmp = Vec::new();
    for bitmap in bitmaps {
        adjacency.push_vector(&bitmap, &mut tmp);
    }
}

pub async fn generate_raw_adjacency() -> Result<()> {
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
            if webpage_count % 200000 == 0 {
                debug!("Preprocessing bitmaps as of {webpage_count}...");
            }
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

    let graph = Arc::new(RawAdjacencyInfo::new(filtered_glyphs.clone()));
    {
        let path = File::open("run/common-crawl_parsed-bitmaps.zst")?;
        let reader = BufReader::new(path);
        let mut zstd = Decoder::new(reader)?;

        let mut i = 0;
        let mut bitmaps = Vec::new();
        let mut threads = Vec::new();
        while let Ok(bitmap) = RoaringBitmap::deserialize_unchecked_from(&mut zstd) {
            bitmaps.push(bitmap);

            i += 1;
            if i % 200000 == 0 {
                debug!("Submitting bitmaps as of {i}/{webpage_count} for processing...");
                let task = tokio::spawn(push_to_table(
                    i,
                    webpage_count,
                    graph.clone(),
                    std::mem::replace(&mut bitmaps, Vec::new()),
                ));
                threads.push(task);
            }
        }
        push_to_table(i, webpage_count, graph.clone(), bitmaps).await;

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
