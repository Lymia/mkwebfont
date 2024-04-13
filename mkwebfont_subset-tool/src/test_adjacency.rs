use std::collections::HashSet;
use crate::generate_adjacency::RawAdjacencyInfo;
use anyhow::Result;

pub fn test_adjacency() -> Result<()> {
    let graph = RawAdjacencyInfo::deserialize("run/common-crawl_adjacency.zst")?;

    let freq_list = {
        let mut info = Vec::new();
        for ch in graph.codepoints() {
            let ch = char::from_u32(*ch).unwrap();
            info.push((ch, graph.get_codepoint_count(ch)));
        }
        info.sort_by_key(|x| x.1);
        info.reverse();

        let mut list = Vec::new();
        for (ch, _) in info {
            list.push(ch);
        }
        list
    };

    let subset_size = 25;
    let mut fulfilled = HashSet::new();
    let mut remaining = HashSet::new();
    remaining.extend(freq_list.iter().map(|x| *x));
    for ch in &freq_list {
        let ch = *ch;
        if !fulfilled.contains(&ch) {
            let mut subset = Vec::new();
            subset.push(ch);
            fulfilled.insert(ch);
            remaining.remove(&ch);

            while subset.len() < subset_size && fulfilled.len() != freq_list.len() {
                let mut best_ct = 0;
                let mut best_ch = None;
                for ch in &remaining {
                    let ch = *ch;
                    let mut accum = 0u64;
                    for ch2 in &subset {
                        accum += graph.get_cooccurance_count(ch, *ch2) as u64;
                    }
                    if accum > best_ct {
                        best_ct = accum;
                        best_ch = Some(ch);
                    }
                }

                let ch = best_ch.unwrap();
                subset.push(ch);
                fulfilled.insert(ch);
                remaining.remove(&ch);
            }

            let mut str = String::new();
            for ch in subset {
                str.push(ch);
            }
            println!("{str:?}");
        }
    }

    Ok(())
}
