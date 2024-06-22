use crate::font_info::FontStyle;
use bincode::{config::standard, Decode, Encode};
use mkwebfont_common::{compression::zstd_decompress, download_cache::DownloadInfo};
use std::{
    fmt::Debug,
    ops::RangeInclusive,
    sync::{Arc, LazyLock},
};

#[derive(Debug, Clone, Decode, Encode)]
pub struct GfontsList {
    pub repo_revision: String,
    pub repo_date: String,
    pub repo_short_date: String,
    pub fonts: Vec<GfontInfo>,
}
impl GfontsList {
    pub fn load() -> Arc<GfontsList> {
        static CACHE: LazyLock<Arc<GfontsList>> = LazyLock::new(|| {
            let data = include_bytes!("gfonts_list.bin.zst");
            let decompressed = zstd_decompress(data).unwrap();
            let out = bincode::decode_from_slice(&decompressed, standard()).unwrap();
            Arc::new(out.0)
        });
        CACHE.clone()
    }
}

#[derive(Debug, Clone, Decode, Encode)]
pub struct GfontInfo {
    pub name: String,
    pub styles: Vec<GfontStyleInfo>,
}

#[derive(Debug, Clone, Decode, Encode)]
pub struct GfontStyleInfo {
    pub style: FontStyle,
    pub weight: RangeInclusive<u32>,
    pub info: DownloadInfo,
}
