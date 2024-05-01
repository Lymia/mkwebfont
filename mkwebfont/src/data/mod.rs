use anyhow::{bail, Result};
use mkwebfont_common::{
    hashing::hash_full,
    model::{
        adjacency_array::AdjacencyArray,
        bitset_list::BitsetList,
        data_package::{DataPackage, KnownHash},
        package_consts::{
            PKG_ADJACENCY_TAG, PKG_GFSUBSETS_TAG, PKG_GLYPHSETS_TAG, PKG_VALIDATION_TAG,
        },
        subset_data::{RawSubsets, WebfontData},
    },
};
use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
};
use tokio::sync::Mutex;
use tracing::{debug, info};

struct DownloadSource {
    name: &'static str,
    cache_name: &'static str,
    timestamp: u64,
    download_url: &'static str,
    download_hash: &'static str,
    known_hash: KnownHash,
}

const BUILTIN_PACKAGE: &[u8] = include_bytes!("mkwebfont-datapkg-builtin-v0.1.0");

const ADJACENCY_DL_INFO: DownloadSource = DownloadSource {
    name: "mkwebfont-datapkg-adjacency-v0.1.0",
    cache_name: "mkwebfont-datapkg-adjacency-v0.1.0_xkcmc0ccmfqfs7s46krs",
    timestamp: 1714591362,
    download_url: "https://github.com/Lymia/mkwebfont/releases/download/mkwebfont-data-v0.1.0/mkwebfont-datapkg-adjacency-v0.1.0",
    download_hash: "0xkcmc0ccmfqfs7s46krswy0avkfmr9b61mcliaygrf3kzqkxjra",
    known_hash: [
        179, 225, 195, 19, 40, 131, 225, 4, 224, 158, 205, 90, 136, 225, 22, 29, 90, 188, 29, 8,
        84, 166, 144, 117, 146, 78, 250, 254, 177, 156, 51, 130,
    ],
};

const VALIDATION_DL_INFO: DownloadSource = DownloadSource {
    name: "mkwebfont-datapkg-validation-v0.1.0",
    cache_name: "mkwebfont-datapkg-validation-v0.1.0_4sayxa8z1s1634g290hg",
    timestamp: 1714591362,
    download_url: "https://github.com/Lymia/mkwebfont/releases/download/mkwebfont-data-v0.1.0/mkwebfont-datapkg-validation-v0.1.0",
    download_hash: "04sayxa8z1s1634g290hgzp39nai21bisl4z1h7qhpnqs4dw792w",
    known_hash: [
        235, 17, 107, 210, 220, 62, 105, 119, 109, 65, 108, 83, 138, 95, 199, 157, 169, 78, 209,
        210, 147, 197, 214, 47, 169, 183, 28, 233, 168, 202, 116, 137,
    ],
};

#[cfg(feature = "download-data")]
async fn data_package_from_source(source: &DownloadSource) -> Result<DataPackage> {
    let cache_dir = directories::ProjectDirs::from("moe.aura", "", "mkwebfont")
        .expect("Could not get cache directory!");
    if !cache_dir.cache_dir().exists() {
        std::fs::create_dir_all(cache_dir.cache_dir())?;
    }
    let cache_dir = cache_dir.cache_dir();

    let mut cache_path = PathBuf::from(cache_dir);
    cache_path.push(source.cache_name);
    debug!("Cached path: '{}' in '{}'", source.name, cache_path.display());

    if cache_path.exists() {
        DataPackage::load_with_hash(cache_path, source.known_hash)
    } else {
        info!("Downloading '{}' from '{}'...", source.name, source.download_url);

        let request = reqwest::get(source.download_url).await?;
        let data = request.bytes().await?.to_vec();

        if hash_full(&data) != source.download_hash {
            bail!("Downloaded package hash does not match.");
        } else {
            std::fs::write(&cache_path, &data)?;
            DataPackage::load_with_hash(cache_path, source.known_hash)
        }
    }
}

#[cfg(feature = "appimage")]
async fn data_package_from_source(source: &DownloadSource) -> Result<DataPackage> {
    let var = std::env::var("MKWEBFONT_APPIMAGE_DATA")?;
    let mut path = PathBuf::from(var);
    path.push(source.name);

    if path.exists() {
        DataPackage::load(path)
    } else {
        bail!("No data found in AppImage!?")
    }
}

async fn pkg_adjacency() -> Result<DataPackage> {
    data_package_from_source(&ADJACENCY_DL_INFO).await
}

async fn pkg_builtin() -> Result<DataPackage> {
    DataPackage::load_mem(BUILTIN_PACKAGE)
}

async fn pkg_validation() -> Result<DataPackage> {
    data_package_from_source(&VALIDATION_DL_INFO).await
}

pub struct DataStorage {
    adjacency_array: Mutex<Option<Arc<AdjacencyArray>>>,
    validation_list: Mutex<Option<Arc<BitsetList>>>,
    gfsubsets: Mutex<Option<Arc<WebfontData>>>,
    glyphsets: Mutex<Option<Arc<WebfontData>>>,
}
impl DataStorage {
    //noinspection DuplicatedCode
    pub async fn adjacency_array(&self) -> Result<Arc<AdjacencyArray>> {
        let mut lock = self.adjacency_array.lock().await;
        if let Some(value) = lock.as_ref() {
            Ok(value.clone())
        } else {
            let mut builtin = pkg_adjacency().await?;
            let value =
                Arc::new(AdjacencyArray::deserialize(builtin.take_section(PKG_ADJACENCY_TAG)?)?);
            *lock = Some(value.clone());
            Ok(value)
        }
    }

    //noinspection DuplicatedCode
    pub async fn validation_list(&self) -> Result<Arc<BitsetList>> {
        let mut lock = self.validation_list.lock().await;
        if let Some(value) = lock.as_ref() {
            Ok(value.clone())
        } else {
            let mut builtin = pkg_validation().await?;
            let value =
                Arc::new(BitsetList::deserialize(builtin.take_section(PKG_VALIDATION_TAG)?)?);
            *lock = Some(value.clone());
            Ok(value)
        }
    }

    //noinspection DuplicatedCode
    pub async fn gfsubsets(&self) -> Result<Arc<WebfontData>> {
        let mut lock = self.gfsubsets.lock().await;
        if let Some(value) = lock.as_ref() {
            Ok(value.clone())
        } else {
            let mut builtin = pkg_builtin().await?;
            let value = Arc::new(
                RawSubsets::deserialize(builtin.take_section(PKG_GFSUBSETS_TAG)?)?.build(),
            );
            *lock = Some(value.clone());
            Ok(value)
        }
    }

    //noinspection DuplicatedCode
    pub async fn glyphsets(&self) -> Result<Arc<WebfontData>> {
        let mut lock = self.glyphsets.lock().await;
        if let Some(value) = lock.as_ref() {
            Ok(value.clone())
        } else {
            let mut builtin = pkg_builtin().await?;
            let value = Arc::new(
                RawSubsets::deserialize(builtin.take_section(PKG_GLYPHSETS_TAG)?)?.build(),
            );
            *lock = Some(value.clone());
            Ok(value)
        }
    }

    pub fn instance() -> Result<Arc<Self>> {
        static ONCE: OnceLock<Arc<DataStorage>> = OnceLock::new();
        let value = ONCE
            .get_or_init(|| {
                Arc::new(DataStorage {
                    adjacency_array: Default::default(),
                    validation_list: Default::default(),
                    gfsubsets: Default::default(),
                    glyphsets: Default::default(),
                })
            })
            .clone();
        Ok(value)
    }
}
