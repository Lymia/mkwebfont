use crate::font_info::{FontStyle, FontWeight};
use bincode::{config::standard, Decode, Encode};
use mkwebfont_common::{
    compression::zstd_decompress, download_cache::DownloadInfo, hashing::WyHashBuilder,
};
use std::{
    collections::HashMap,
    fmt::{Debug, Display, Formatter},
    ops::RangeInclusive,
    sync::LazyLock,
};

#[derive(Debug, Clone, Decode, Encode)]
pub struct GfontsList {
    pub repo_revision: String,
    pub repo_date: String,
    pub repo_short_date: String,
    pub fonts: Vec<GfontInfo>,
}
impl GfontsList {
    pub fn load() -> &'static GfontsList {
        static CACHE: LazyLock<GfontsList> = LazyLock::new(|| {
            let data = include_bytes!("gfonts_list.bin.zst");
            let decompressed = zstd_decompress(data).unwrap();
            let out = bincode::decode_from_slice(&decompressed, standard()).unwrap();
            out.0
        });
        &*CACHE
    }

    pub fn find_font(name: &str) -> Option<&'static GfontInfo> {
        static CACHE: LazyLock<HashMap<&'static str, &'static GfontInfo, WyHashBuilder>> =
            LazyLock::new(|| {
                let mut map = HashMap::default();
                for font in &GfontsList::load().fonts {
                    map.insert(font.name.as_str(), font);
                }
                map
            });
        CACHE.get(name.to_lowercase().as_str()).cloned()
    }
}

#[derive(Debug, Clone, Decode, Encode)]
pub struct GfontInfo {
    pub name: String,
    pub styles: Vec<GfontStyleInfo>,
}
impl GfontInfo {
    pub fn find_nearest_match(
        &self,
        style: FontStyle,
        weight: FontWeight,
    ) -> Option<&GfontStyleInfo> {
        if let Some((info, _)) = self
            .styles
            .iter()
            .filter(|x| x.style.is_compatible(style))
            .map(|x| (x, (x.style == style, weight.dist_from_range(&x.weight))))
            .min_by_key(|x| x.1)
        {
            Some(info)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Decode, Encode)]
pub struct GfontStyleInfo {
    pub style: FontStyle,
    pub weight: RangeInclusive<u32>,
    pub info: DownloadInfo,
}
impl Display for GfontStyleInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let start = FontWeight::from_num(*self.weight.start());
        let end = FontWeight::from_num(*self.weight.end());
        if start == end {
            write!(f, "{} / {start}", self.style)
        } else {
            write!(f, "{} / {start} to {end}", self.style)
        }
    }
}
