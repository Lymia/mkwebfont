//! A (very poorly written) script to scrape the character split classes used by Google Fonts to
//! a Rust data file.
//!
//! Code quality is very bad, but this needs to be run very rarely, so... it shouldn't matter much.

use anyhow::*;
use roaring::RoaringBitmap;
use serde::*;
use std::{borrow::Cow, collections::HashMap, fs::File, io::Write};
use tracing::info;
use unic_ucd_category::GeneralCategory;

/// Really shitty CSS parser
fn parse_css_poorly(css: &str, cjk_tag: &str) -> Result<HashMap<String, RoaringBitmap>> {
    let mut current_subset = None;
    let mut ranges = HashMap::new();
    for line in css.split("\n") {
        let line = line.trim();

        // the line is one of the comments
        if line.starts_with("/*") {
            let line = line.split("/*").skip(1).next().unwrap().trim();
            let line = line.split("*/").next().unwrap().trim();
            current_subset = Some(Cow::Borrowed(line));

            if line.starts_with("[") && line.ends_with("]") {
                let line = line.split("[").skip(1).next().unwrap().trim();
                let line = line.split("]").next().unwrap().trim();
                current_subset = Some(Cow::Owned(format!("group-{cjk_tag}-s{line}")));
            }
        }

        // the line is a unicode range
        if line.starts_with("unicode-range: ") {
            let subset = current_subset.take().unwrap();

            let line = line.split(":").skip(1).next().unwrap().trim();
            let line = line.split(";").next().unwrap().trim();

            let mut chars = RoaringBitmap::new();
            for entry in line.split(",") {
                let entry = entry.trim();
                assert!(entry.starts_with("U+"), "entry does not start with U+");
                let entry = &entry[2..];

                if entry.contains('-') {
                    let split: Vec<_> = entry.split("-").collect();
                    assert_eq!(split.len(), 2);
                    let start = u32::from_str_radix(split[0], 16)?;
                    let end = u32::from_str_radix(split[1], 16)?;
                    for ch in start..=end {
                        chars.insert(ch);
                    }
                } else {
                    let ch = u32::from_str_radix(entry, 16)?;
                    chars.insert(ch);
                }
            }
            ranges.insert(subset.to_string(), chars);
        }
    }
    Ok(ranges)
}

async fn mk_gf_ranges() -> Result<()> {
    // download the font list
    let webfont_apikey = std::env::var("WEBFONT_APIKEY")?;
    let client = reqwest::ClientBuilder::new()
        .user_agent("Mozilla/5.0 (X11; Linux x86_64; rv:124.0) Gecko/20100101 Firefox/124.0")
        .build()?;
    let font_list = client
        .get(format!("https://www.googleapis.com/webfonts/v1/webfonts?key={webfont_apikey}"))
        .send()
        .await?;
    let fonts: WebfontsIndex = serde_json::from_str(&font_list.text().await?)?;

    // download and parse all fonts
    #[derive(Clone, Serialize, Deserialize)]
    struct WebfontsIndex {
        items: Vec<WebfontsEntry>,
    }

    #[derive(Clone, Serialize, Deserialize)]
    struct WebfontsEntry {
        family: String,
        subsets: Vec<String>,
    }

    let mut raw_subsets: HashMap<_, RoaringBitmap> = HashMap::new();
    for font in fonts.items {
        info!("Getting CSS for {}...", font.family);

        let has_chinese_simplified = font.subsets.iter().any(|x| x == "chinese-simplified");
        let has_chinese_traditional = font.subsets.iter().any(|x| x == "chinese-traditional");
        let has_chinese_hongkong = font.subsets.iter().any(|x| x == "chinese-hongkong");
        let has_korean = font.subsets.iter().any(|x| x == "korean");
        let has_japanese = font.subsets.iter().any(|x| x == "japanese");
        let has_emoji = font.subsets.iter().any(|x| x == "emoji");
        let is_multiple = (has_chinese_simplified as u8)
            + (has_chinese_traditional as u8)
            + (has_chinese_hongkong as u8)
            + (has_korean as u8)
            + (has_japanese as u8)
            + (has_emoji as u8)
            > 1;

        let cjk_tag = if is_multiple {
            "unk"
        } else if has_chinese_simplified {
            "zhsimp"
        } else if has_chinese_traditional {
            "zhtrad"
        } else if has_chinese_hongkong {
            "zhhk"
        } else if has_korean {
            "kr"
        } else if has_japanese {
            "jp"
        } else if has_emoji {
            "emoji"
        } else {
            "unk"
        };

        let font_css = client
            .get(format!("https://fonts.googleapis.com/css2?family={}", &font.family))
            .send()
            .await?;
        let parsed = parse_css_poorly(&font_css.text().await?, cjk_tag)?;

        for (k, v) in parsed {
            if let Some(subset) = raw_subsets.get_mut(&k) {
                if *subset != v {
                    info!(
                        "{k} - merging {} codepoints with {} codepoints",
                        subset.len(),
                        v.len()
                    );
                    subset.extend(v);
                }
            } else {
                raw_subsets.insert(k, v);
            }
        }
    }

    // check for ranges with multiple definitions and merge them
    let mut names = Vec::new();
    for (k, v) in &raw_subsets {
        info!("{k}: {} codepoints", v.len());
        names.push(k.clone());
    }
    names.sort();

    // sort into the Google Fonts machine learning subsets and manually coded subsets
    struct SubsetInfo {
        name: String,
        group: Option<String>,
        chars: RoaringBitmap,
    }

    let mut grouped_subsets = Vec::new();
    for name in names {
        let chars = raw_subsets.remove(&name).unwrap();
        let mut subset = SubsetInfo { name: name.clone(), group: None, chars };

        if name.starts_with("group-") {
            let subclass = name.split('-').skip(1).next().unwrap();
            subset.name = subset.name[6..].to_string().replace("-s", "");
            subset.group = Some(subclass.to_string());
        }

        grouped_subsets.push(subset);
    }

    // output the data file
    #[derive(Serialize)]
    struct Subset {
        name: String,
        group: Option<String>,
        chars: String,
        codepoints: Vec<u32>,
    }
    #[derive(Serialize)]
    struct Subsets {
        subset: Vec<Subset>,
    }

    let mut subsets = Vec::new();
    for subset in grouped_subsets {
        let mut chars = String::new();
        let mut codepoints = Vec::new();
        for char in subset.chars {
            let cat = GeneralCategory::of(char::from_u32(char).unwrap());
            if cat.is_letter() || cat.is_number() || cat.is_punctuation() || cat.is_symbol() {
                chars.push(char::from_u32(char).unwrap())
            } else {
                codepoints.push(char);
            }
        }

        subsets.push(Subset { name: subset.name, group: subset.group, chars, codepoints });
    }
    let subsets = Subsets { subset: subsets };

    let mut file = File::create("mkwebfont/src/subset_manifest_default.toml")?;
    writeln!(file, "# ")?;
    writeln!(file, "# Subset Manifest")?;
    writeln!(file, "# ===============")?;
    writeln!(file, "# ")?;
    writeln!(file, "# This file defines the subsets that mkwebfont splits its fonts into.")?;
    writeln!(file, "# Each [[subset]] block has the following fields:")?;
    writeln!(file, "# ")?;
    writeln!(file, "# * name       = the name of the subset")?;
    writeln!(file, "# * group      = the optional name of the subset group")?;
    writeln!(file, "# * chars      = a string containing unicode codepoints to include")?;
    writeln!(file, "# * codepoints = a list of unicode codepoints to include")?;
    writeln!(file, "# ")?;
    writeln!(file, "# Subsets in a group will only be generated together or not at all.")?;
    writeln!(file, "# ")?;
    writeln!(file, "# The default subset manifest here is based on Google Fonts metadata.")?;
    writeln!(file, "# Regenerate it with `cargo run -p mkwebfont_subset-tool`.")?;
    writeln!(file, "# ")?;
    writeln!(file)?;
    writeln!(file)?;
    file.write_all(toml::to_string(&subsets)?.as_bytes())?;
    Ok(())
}

pub async fn main() {
    mk_gf_ranges().await.unwrap();
}
