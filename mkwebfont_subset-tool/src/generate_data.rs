use crate::raw_adjacency::RawAdjacencyInfo;
use anyhow::Result;
use log::info;
use mkwebfont_common::{
    adjacency_bloom_filter::{AdjacencyBloomFilter, BloomFilterBuilder, CodepointInfo},
    data_package::{DataPackage, DataPackageEncoder},
    join_set::JoinSet,
    wyhash::WyRand,
};
use std::{
    collections::HashMap,
    sync::{atomic::Ordering, Arc},
};
use tracing::debug;

pub const VERSION: &str = "v0.1.3";

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
        let codepoints: Vec<_> = graph.codepoint_list.iter().cloned().enumerate().collect();

        let mut block_ids = HashMap::new();
        for &codepoint in graph.codepoint_list.iter() {
            let block = unic_ucd_block::Block::of(char::from_u32(codepoint).unwrap()).unwrap();
            if block_ids.get(block.name).is_none() {
                let id = block_ids.len() as u32;
                block_ids.insert(block.name, id);
            };
        }

        let mut join_set: JoinSet<Vec<_>> = JoinSet::new();
        let graph = graph.clone();
        join_set.map_vec("codepoints", &codepoints, 400, move |&(i, codepoint)| {
            let mut edge_total = 0.0;
            let mut edge_maximum = 0;
            let mut edges = Vec::new();
            for j in 0..graph.codepoint_list.len() {
                if i != j {
                    let value = graph.get_cooccurance_count(codepoint, graph.codepoint_list[j]);
                    edge_total += value as f64;
                    edge_maximum = edge_maximum.max(value as u64);
                    edges.push(value);
                }
            }

            let edge_len = edges.len();
            let edge_median = *edges.select_nth_unstable(edge_len / 2).1 as u64;

            let count = graph.get_codepoint_count(codepoint);
            let block = unic_ucd_block::Block::of(char::from_u32(codepoint).unwrap()).unwrap();
            let block_id = *block_ids.get(block.name).unwrap();

            Ok((codepoint, CodepointInfo {
                count: count as u64,
                edge_total,
                edge_median,
                edge_maximum,
                block_id,
            }))
        });
        for (codepoint, info) in join_set.join_vec().await? {
            glyphs.insert(codepoint, info);
        }
    }

    let mut total = 0;
    let mut list = Vec::new();
    for &glyph in glyphs.keys() {
        list.push(glyph);
    }
    list.sort();
    for &cha in &list {
        let mut list2 = Vec::new();
        let mut gz = 0;
        let a_ct = glyphs.get(&cha).unwrap().edge_total;
        for &chb in &list {
            if cha != chb {
                let b_ct = glyphs.get(&chb).unwrap().edge_total;
                let expectation = (a_ct * b_ct) / (2.0 * edge_total);
                let modularity = graph.get_cooccurance_count(cha, chb) as f64 - expectation;

                list2.push((chb, (modularity * 100000.0) as u64, modularity));

                if modularity >= 10.0 {
                    gz += 1;
                }
            }
        }

        list2.sort_by_key(|x| x.1);
        list2.reverse();

        let ct = 300;
        let sx = list2[0].2;
        let sy = list2[ct - 1].2;
        let mut str2 = String::new();
        for (nch, _, _) in list2.into_iter().take(ct) {
            str2.push(char::from_u32(nch).unwrap());
        }

        let cha = char::from_u32(cha).unwrap();
        println!("(mod) {cha:?} / {sx:13.4} / {sy:13.4} / {gz:5} / {str2:?}");
        total += gz;
    }
    println!("total: {total}");

    let bloom = Arc::new(BloomFilterBuilder::new(
        VERSION,
        (1 << 20) * 512,
        8,
        glyphs,
        1.5,
        edge_total,
        (graph.data.len() - graph.codepoint_list.len()) as f64,
    ));
    {
        let mut join_set: JoinSet<Vec<_>> = JoinSet::new();
        let range: Vec<_> = (0..graph.codepoints().len()).collect();
        let graph = graph.clone();
        let bloom = bloom.clone();
        join_set.map_vec("encoding chunk", &range, 400, move |&i| {
            let mut idx = crate::raw_adjacency::place_idx(i, i);
            for j in (0..=i).into_iter().rev() {
                if i != j {
                    let a = graph.codepoint_list[i];
                    let b = graph.codepoint_list[j];

                    let cooccurance = graph.data[idx].load(Ordering::Relaxed);
                    if cooccurance != 0 {
                        bloom.insert_pairing(a, b, cooccurance as u64);
                    }
                }
                idx += 1;
            }
            Ok(())
        });

        join_set.join().await?;
    }
    let bloom = bloom.finish();

    info!("Checking accuracy...");
    let mut rng = WyRand::new(20000);
    let test_count = 50000000;
    let mut diff_absolute = 0.0;
    let mut maximum_error = 0.0f64;
    let mut maximum_error_ratio = 0.0f64;
    for _ in 0..test_count {
        let cpl = &graph.codepoint_list;
        let g1 = cpl[rng.rand() as usize % cpl.len()];
        let g2 = cpl[rng.rand() as usize % cpl.len()];

        let cor = graph.get_cooccurance_count(g1, g2) as f64;
        let blo = bloom.get_pairing(g1, g2) as f64;

        diff_absolute += (cor - blo).abs();
        maximum_error = maximum_error.max((cor - blo).abs());
        if cor != 0.0 {
            maximum_error_ratio = maximum_error_ratio.max(((blo / cor) - 1.0).abs());
        }
    }
    info!("Average error: +{:.4}", diff_absolute / test_count as f64);
    info!("Maximum error: {:.4}, +{:.2}%", maximum_error, maximum_error_ratio * 100.0);

    let bloom = bloom.with_average_error(diff_absolute / test_count as f64);
    bloom.serialize(data_encoder, "main")?;

    Ok(())
}

async fn check_packages() -> Result<()> {
    info!("Checking generated packages...");

    let data = std::fs::read(format!("run/mkwebfont_data-{VERSION}"))?;
    let data = DataPackage::deserialize(&data)?;

    debug!("Testing adjacency filter decoding...");
    AdjacencyBloomFilter::deserialize(&data, "main")?;

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
