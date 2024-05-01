use anyhow::Result;
use mkwebfont_common::model::{
    data_package::DataPackageEncoder,
    subset_data::{RawSubset, RawSubsets},
};
use tracing::debug;

pub const GLYPHSETS_PATH: &str = "run/raw_glyphsets";
pub const GLYPHSETS_TAG: &str = "glyphsets";
const GLYPHSETS_VERSION: &str = "v0.1.0";

const URL_PREFIX: &str =
    "https://raw.githubusercontent.com/googlefonts/glyphsets/main/data/results/nam/";
const GLYPH_SETS: &[(&str, &str)] = &[
    ("arb", "GF_Arabic_Core.nam"),
    ("arbx", "GF_Arabic_Plus.nam"),
    ("cry", "GF_Cyrillic_Core.nam"),
    ("cryx", "GF_Cyrillic_Plus.nam"),
    ("cryz", "GF_Cyrillic_Pro.nam"),
    ("grk", "GF_Greek_Core.nam"),
    ("grkx", "GF_Greek_Plus.nam"),
    ("lat", "GF_Latin_Core.nam"),
    ("lat_af", "GF_Latin_African.nam"),
    ("lat_vi", "GF_Latin_Vietnamese.nam"),
];

pub async fn load_subset(src: &str) -> Result<String> {
    debug!("Processing subset: {src:?}");

    let src = format!("{URL_PREFIX}{src}");
    let result = reqwest::get(src).await?;
    let text = result.text().await?;

    let mut chars = Vec::new();
    for line in text.split("\n") {
        if line.starts_with("0x") {
            let code = line[2..].split(' ').next().unwrap();
            chars.push(char::from_u32(u32::from_str_radix(code, 16)?).unwrap());
        }
    }
    chars.sort();

    let mut str = String::new();
    for ch in chars {
        str.push(ch)
    }
    Ok(str)
}

pub async fn generate_glyphsets() -> Result<()> {
    let mut subsets = Vec::new();
    for (name, src) in GLYPH_SETS {
        let subset = load_subset(src).await?;
        subsets.push(RawSubset { name: name.to_string(), group: None, chars: subset });
    }
    let subsets = RawSubsets { subsets };

    let name = format!("{GLYPHSETS_TAG}/{GLYPHSETS_VERSION}");
    let mut package = DataPackageEncoder::new(&name);
    package.insert_section(GLYPHSETS_TAG, subsets.serialize(&name)?);
    package.build().save(GLYPHSETS_PATH)?;

    Ok(())
}
