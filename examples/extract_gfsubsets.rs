//! A (very poorly written) script to scrape the character split classes used by Google Fonts to
//! a Rust data file.

use anyhow::*;
use serde::*;
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    fmt::{Display, Formatter},
    fs::File,
    io::Write,
    ops::RangeInclusive,
};
use unic_ucd_category::GeneralCategory;

/// Really shitty CSS parser
fn parse_css_poorly(css: &str, cjk_tag: &str) -> Result<HashMap<String, Vec<char>>> {
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
                current_subset = Some(Cow::Owned(format!("gf-{cjk_tag}-s{line}")));
            }
        }

        // the line is a unicode range
        if line.starts_with("unicode-range: ") {
            let subset = current_subset.take().unwrap();

            let line = line.split(":").skip(1).next().unwrap().trim();
            let line = line.split(";").next().unwrap().trim();

            let mut chars = HashSet::new();
            for entry in line.split(",") {
                let entry = entry.trim();
                assert!(entry.starts_with("U+"), "entry does not start with U+");
                let entry = &entry[2..];

                if entry.contains('-') {
                    let split: Vec<_> = entry.split("-").collect();
                    assert_eq!(split.len(), 2);
                    let start = u32::from_str_radix(split[0], 16)?;
                    let end = u32::from_str_radix(split[1], 16)?;
                    for i in start..=end {
                        chars.insert(char::from_u32(i).unwrap());
                    }
                } else {
                    let c = u32::from_str_radix(entry, 16)?;
                    chars.insert(char::from_u32(c).unwrap());
                }
            }

            let mut chars: Vec<_> = chars.into_iter().collect();
            chars.sort();
            ranges.insert(subset.to_string(), chars);
        }
    }
    Ok(ranges)
}

/// Turn a list of character codes into a list of ranges.
fn unify_ranges(mut range: Vec<char>) -> Vec<RangeInclusive<char>> {
    range.sort();

    let mut range_start = None;
    let mut range_last = '\u{fffff}';
    let mut ranges = Vec::new();
    for char in range {
        if let Some(start) = range_start {
            let next = char::from_u32(range_last as u32 + 1).unwrap();
            if next != char {
                ranges.push(start..=range_last);
                range_start = Some(char);
            }
        } else {
            range_start = Some(char);
        }
        range_last = char;
    }
    if let Some(start) = range_start {
        ranges.push(start..=range_last);
    }

    ranges
}

fn main_impl() -> Result<()> {
    // download the font list
    let webfont_apikey = std::env::var("WEBFONT_APIKEY")?;
    let client = reqwest::blocking::ClientBuilder::new()
        .user_agent("Mozilla/5.0 (X11; Linux x86_64; rv:124.0) Gecko/20100101 Firefox/124.0")
        .build()?;
    let font_list = client
        .get(format!("https://www.googleapis.com/webfonts/v1/webfonts?key={webfont_apikey}"))
        .send()?;
    let fonts: WebfontsIndex = serde_json::from_str(&font_list.text()?)?;

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

    let mut ranges = HashMap::new();
    let mut sources: HashMap<_, HashSet<_>> = HashMap::new();
    for font in fonts.items {
        println!("Getting CSS for {}...", font.family);

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
            "zh_simp"
        } else if has_chinese_traditional {
            "zh_trad"
        } else if has_chinese_hongkong {
            "zh_hk"
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
            .send()?;
        let parsed = parse_css_poorly(&font_css.text()?, cjk_tag)?;
        for (k, v) in parsed {
            sources
                .entry(k.clone())
                .or_default()
                .insert(font.family.clone());
            ranges.insert(v, k);
        }
    }

    // check for ranges with multiple definitions and merge them
    let mut ranges: Vec<_> = ranges.into_iter().collect();
    ranges.sort_by_key(|x| x.1.clone());

    let mut ordered_classes = Vec::new();
    let mut overlap_check = HashSet::new();
    let mut classes = HashMap::new();
    for (k, v) in ranges {
        println!("{v} - {} glyphs - used in {} fonts", k.len(), sources.get(&v).unwrap().len());
        if overlap_check.contains(&v) {
            println!("!!!!!!!!!!!!!!!!!!! Category has more than one definitions: {v:?}");
            let old = classes.get_mut(&v).unwrap();
            let old_vec = std::mem::replace(old, Vec::new());

            let mut set = HashSet::new();
            set.extend(k.into_iter());
            set.extend(old_vec.into_iter());

            let mut new_vec: Vec<_> = set.into_iter().collect();
            new_vec.sort();

            *old = new_vec;

            println!("{v} - merged: {} glyphs", old.len());
        } else {
            overlap_check.insert(v.clone());
            ordered_classes.push(v.clone());
            classes.insert(v, k);
        }
    }

    // sort into the Google Fonts machine learning subsets and manually coded subsets
    struct GfSubset {
        name: String,
        ranges: Vec<RangeInclusive<char>>,
    }

    let mut subsets = Vec::new();
    let mut gf_subsets: HashMap<_, Vec<_>> = HashMap::new();
    for class in ordered_classes {
        let class_data = classes.remove(&class).unwrap();
        let subset = GfSubset {
            name: class.clone(),
            ranges: unify_ranges(class_data),
        };

        if class.starts_with("gf-") {
            let subclass = class.split('-').skip(1).next().unwrap();
            gf_subsets
                .entry(subclass.to_string())
                .or_default()
                .push(subset);
        } else {
            subsets.push(subset);
        }
    }

    // output the data file
    let mut file = File::create("src/gf_ranges.rs")?;
    fn write_subset(file: &mut File, subset: &GfSubset) -> Result<()> {
        struct CharRepr(char);
        impl Display for CharRepr {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                let category = GeneralCategory::of(self.0);
                if self.0 == '\'' {
                    write!(f, "'\\''")
                } else if self.0 == ' ' {
                    write!(f, "' '")
                } else if category.is_letter()
                    || category.is_number()
                    || category.is_punctuation()
                    || category.is_symbol()
                {
                    write!(f, "'{}'", self.0)
                } else {
                    write!(f, "'{}'", self.0.escape_unicode())
                }
            }
        }

        write!(file, "GfSubset{{name:{:?},ranges:&[", subset.name)?;
        for range in &subset.ranges {
            if range.start() == range.end() {
                write!(file, "o({}),", CharRepr(*range.start()))?;
            } else {
                write!(file, "{}..={},", CharRepr(*range.start()), CharRepr(*range.end()))?;
            }
        }
        writeln!(file, "],}},")?;

        Ok(())
    }
    fn write_subsets(file: &mut File, subsets: &[GfSubset]) -> Result<()> {
        writeln!(file, "// -- start {} ranges --", subsets.len())?;
        for subset in subsets {
            write_subset(file, subset)?;
        }
        writeln!(file, "// -- end {} ranges --", subsets.len())?;
        Ok(())
    }
    writeln!(file, "// Automatically generated file. Do not edit manually.")?;
    writeln!(file, "// Run `cargo run --example extract_gfsubsets`.`")?;
    writeln!(file, "#![cfg_attr(rustfmt, rustfmt_skip)]")?;
    writeln!(file, "{}", include_str!("res/gfsubsets.rs"))?;
    writeln!(file, "const DATA: GfSubsets = GfSubsets {{")?;
    writeln!(file, "    subsets: &[")?;
    write_subsets(&mut file, &subsets)?;
    writeln!(file, "    ],")?;
    writeln!(file, "    subset_groups: &[")?;
    for (name, subsets) in gf_subsets {
        writeln!(file, "        GfSubsetGroup {{")?;
        writeln!(file, "            name: {:?},", name)?;
        writeln!(file, "            subsets: &[")?;
        write_subsets(&mut file, &subsets)?;
        writeln!(file, "            ],")?;
        writeln!(file, "        }},")?;
    }
    writeln!(file, "    ],")?;
    writeln!(file, "}};")?;

    writeln!(file, "impl GfSubsets {{")?;
    writeln!(file, "    pub const DATA: GfSubsets = DATA;")?;
    writeln!(file, "}}")?;

    Ok(())
}

fn main() {
    main_impl().unwrap();
}
