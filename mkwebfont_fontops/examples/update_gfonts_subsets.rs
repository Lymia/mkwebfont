use anyhow::Result;
use bincode::config;
use mkwebfont_common::compression::zstd_compress;
use mkwebfont_fontops::gfonts::gfonts_subsets::{RawSubset, RawSubsets};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, io};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct TomlRawSubset {
    name: String,
    group: Option<String>,
    chars: String,
    codepoints: Vec<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct TomlRawSubsets {
    subset: Vec<TomlRawSubset>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(io::stderr)
        .init();

    let toml: TomlRawSubsets = toml::from_str(include_str!("gfonts_subsets.toml"))?;

    let mut reencoded = RawSubsets { subsets: vec![] };
    for subset in &toml.subset {
        let mut chars = HashSet::new();
        chars.extend(subset.chars.chars());
        chars.extend(
            subset
                .codepoints
                .iter()
                .map(|x| char::from_u32(*x).unwrap()),
        );
        let mut chars: Vec<_> = chars.into_iter().collect();
        chars.sort();

        let mut str = String::new();
        for char in chars {
            str.push(char);
        }

        reencoded.subsets.push(RawSubset {
            name: subset.name.clone(),
            group: subset.group.clone(),
            chars: str,
        });
    }

    std::fs::write(
        "mkwebfont_fontops/src/gfonts/gfonts_subsets.bin.zst",
        zstd_compress(&bincode::encode_to_vec(reencoded, config::standard())?)?,
    )?;

    Ok(())
}
