use crate::{
    font_info::{FontFaceWrapper, FontStyle},
    gfonts::gfonts_list::GfontsList,
};
use anyhow::Result;
use mkwebfont_common::{
    character_set::{CharacterSet, CompressedCharacterSet},
    compression::brotli_decompress,
    download_cache::DownloadInfo,
    join_set::JoinSet,
};
use rkyv::{Archive, Deserialize, Serialize};
use std::sync::{Arc, LazyLock};
use tracing::info;

#[derive(Debug, Clone, Deserialize, Serialize, Archive)]
pub struct FallbackInfo {
    pub fonts: Vec<FallbackComponent>,
}
impl FallbackInfo {
    pub fn load<'a>() -> &'a FallbackInfo {
        static CACHE: LazyLock<FallbackInfo> = LazyLock::new(|| {
            let data = include_bytes!("fallback_info.bin.br");
            let decompressed = brotli_decompress(data).unwrap();

            let archived =
                rkyv::access::<ArchivedFallbackInfo, rkyv::rancor::Error>(&decompressed).unwrap();
            rkyv::deserialize::<FallbackInfo, rkyv::rancor::Error>(archived).unwrap()
        });
        &*CACHE
    }

    pub fn build_stack(chars: &CharacterSet) -> Vec<String> {
        let mut chars = chars.clone();
        let loaded = Self::load::<'static>();

        let mut list = Vec::new();
        for font in &loaded.fonts {
            let new_chars = &chars - CharacterSet::decompress(&font.codepoints);
            if new_chars != chars {
                chars = new_chars;
                list.push(font.name.clone())
            }
        }
        list
    }

    pub async fn load_needed_fonts(chars: &CharacterSet) -> Result<Vec<FontFaceWrapper>> {
        let mut chars = chars.clone();
        let loaded = Self::load::<'static>();

        let mut joins = JoinSet::new();
        for font in &loaded.fonts {
            let new_chars = &chars - CharacterSet::decompress(&font.codepoints);
            if new_chars != chars {
                info!("Loading font: (Fallback) {}", font.name);
                chars = new_chars;
                joins.spawn(font.load());
            }
        }
        let files = joins.join_vec().await?;

        let mut joins = JoinSet::new();
        for file in files {
            joins.spawn(async move { FontFaceWrapper::load(None, file) });
        }
        joins.join_vec().await
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Archive)]
pub struct FallbackComponent {
    pub name: String,
    pub source: FallbackDownloadSource,
    pub codepoints: CompressedCharacterSet,
}
impl FallbackComponent {
    pub async fn load(&self) -> Result<Vec<Arc<[u8]>>> {
        let mut results = Vec::new();
        match &self.source {
            FallbackDownloadSource::GFonts(font) => {
                for style in &GfontsList::find_font(&font).unwrap().styles {
                    if style.style == FontStyle::Regular {
                        results.push(style.info.load().await?);
                    }
                }
            }
            FallbackDownloadSource::Download(info) => {
                for dl in info {
                    results.push(dl.load().await?);
                }
            }
        }
        Ok(results)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Archive)]
pub enum FallbackDownloadSource {
    GFonts(String),
    Download(Vec<DownloadInfo>),
}
