use crate::raw_adjacency::RawAdjacencyInfo;
use anyhow::Result;
use log::info;
use mkwebfont_common::{
    adjacency_bloom_filter::{AdjacencyBloomFilter, BloomFilterBuilder, GlyphInfo},
    data_package::{DataPackage, DataPackageEncoder},
    join_set::JoinSet,
};
use rand::seq::SliceRandom;
use std::{
    collections::HashMap,
    sync::{atomic::Ordering, Arc},
};
use tracing::debug;

pub const VERSION: &str = "v0.1.0";

async fn encode_adjacency(data_encoder: &mut DataPackageEncoder) -> Result<()> {
    let graph = Arc::new(RawAdjacencyInfo::deserialize("run/common-crawl_adjacency.zst")?);

    let mut edge_total = 0.0;

    {
        let mut ct_a = 0;
        let mut ct_b = 0;
        for cooccurance in &graph.data {
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

            let cooccurance = cooccurance.load(Ordering::Relaxed);
            edge_total += cooccurance as f64;
        }
    }

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

    let bloom = Arc::new(BloomFilterBuilder::new(
        glyphs,
        1.5,
        edge_total,
        (graph.data.len() - graph.codepoint_list.len()) as f64,
    ));
    {
        let mut remaining = graph.codepoints().len();
        let mut join_set = JoinSet::new();
        while remaining > 0 {
            let chunk_size = if remaining > 400 { 400 } else { remaining };
            remaining -= chunk_size;

            let i_range = remaining..remaining + chunk_size;
            let graph = graph.clone();
            let bloom = bloom.clone();
            join_set.spawn(async move {
                debug!(
                    "Encoding chunk: {}-{}/{}",
                    i_range.start,
                    i_range.end,
                    graph.codepoints().len()
                );
                let mut idx = crate::raw_adjacency::place_idx(i_range.start, i_range.start);
                for i in i_range {
                    for j in (0..=i).into_iter().rev() {
                        if i != j {
                            let cooccurance = graph.data[idx].load(Ordering::Relaxed);
                            bloom.insert_pairing(
                                graph.codepoint_list[i],
                                graph.codepoint_list[j],
                                cooccurance,
                            );
                        }
                        idx += 1;
                    }
                }
                Ok(())
            });
        }
        join_set.join().await?;
    }
    let bloom = bloom.finish();
    bloom.serialize(data_encoder)?;

    info!("Checking accuracy...");
    let mut rng = rand::thread_rng();
    let test_count = 10000000;
    let mut diff_absolute = 0.0;
    let mut maximum_error = 0.0f64;
    let mut maximum_error_ratio = 0.0f64;
    for _ in 0..test_count {
        let g1 = char::from_u32(*graph.codepoint_list.choose(&mut rng).unwrap()).unwrap();
        let g2 = char::from_u32(*graph.codepoint_list.choose(&mut rng).unwrap()).unwrap();

        let cor = graph.get_cooccurance_count(g1, g2) as f64;
        let blo = bloom.load_pairing(g1 as u32, g2 as u32) as f64;

        diff_absolute += (cor - blo).abs();
        maximum_error = maximum_error.max((cor - blo).abs());
        if cor != 0.0 {
            maximum_error_ratio = maximum_error_ratio.max(((blo / cor) - 1.0).abs());
        }
    }
    info!("Average error: +{:.4}", diff_absolute / test_count as f64);
    info!("Maximum error: {:.4}, +{:.2}%", maximum_error, maximum_error_ratio * 100.0);

    Ok(())
}

async fn check_packages() -> Result<()> {
    info!("Checking generated packages...");

    let data = std::fs::read(format!("run/mkwebfont_data-{VERSION}"))?;
    let data = DataPackage::deserialize(&data)?;

    debug!("Testing adjacency filter decoding...");
    AdjacencyBloomFilter::deserialize(&data)?;

    Ok(())
}

pub async fn generate_data() -> Result<()> {
    let mut data_encoder = DataPackageEncoder::new(&format!("mkwebfont_data-{VERSION}"));
    encode_adjacency(&mut data_encoder).await?;

    let data = data_encoder.build();
    std::fs::write(format!("run/mkwebfont_data-{VERSION}"), data.encode()?)?;

    check_packages().await?;

    Ok(())
}
