use crate::raw_adjacency::RawAdjacencyInfo;
use anyhow::Result;
use mkwebfont_common::adjacency_bloom_filter::{AdjacencyBloomFilter, FilterInfo, GlyphInfo};
use std::{collections::HashMap, sync::atomic::Ordering};
use log::info;
use tracing::debug;

pub fn process_adjacency() -> Result<()> {
    let graph = RawAdjacencyInfo::deserialize("run/common-crawl_adjacency.zst")?;

    let mut min = u32::MAX;
    let mut max = u32::MIN;
    let mut edge_total = 0.0;

    {
        let mut ct_a = 0;
        let mut ct_b = 0;
        for cooccurance in &graph.data {
            let cooccurance = cooccurance.load(Ordering::Relaxed);
            min = min.min(cooccurance);
            max = max.max(cooccurance);

            // count triangle numbers
            let is_node = ct_a == 0;
            if ct_a == ct_b {
                ct_a = 0;
                ct_b += 1;
            } else {
                ct_a += 1;
            }
            if is_node {
                continue;
            }

            edge_total += cooccurance as f64;
        }
    }

    let info = FilterInfo::init_for_min_max(
        1.5,
        min,
        max,
        edge_total,
        graph.data.len() as f64,
    );
    debug!("Filter info: {info:?}");

    let mut glyphs = HashMap::new();
    {
        let mut i = 0;
        for (j, glyph) in graph.codepoint_list.iter().enumerate() {
            let ch = char::from_u32(*glyph).unwrap();

            let mut edge_total = 0.0;
            for j in i + 1..=i + j {
                edge_total += graph.data[j].load(Ordering::Relaxed) as f64;
            }
            i += j + 1;

            glyphs.insert(ch, GlyphInfo { count: graph.get_codepoint_count(ch), edge_total });
        }
    }

    let mut bloom = AdjacencyBloomFilter::new(glyphs, info);

    let mut idx = 0;
    for i in 0..graph.codepoints().len() {
        if i != 0 && i % 1000 == 0 {
            debug!("Encoding Progress: {i}/{}", graph.codepoints().len());
        }
        for j in (0..=i).into_iter().rev() {
            let cooccurance = graph.data[idx].load(Ordering::Relaxed);
            bloom.insert_pairing(graph.codepoint_list[i], graph.codepoint_list[j], cooccurance);
            idx += 1;
        }
    }
    bloom.serialize_to_dir("run/adjacency")?;

    info!("Checking accuracy...");


    Ok(())
}
