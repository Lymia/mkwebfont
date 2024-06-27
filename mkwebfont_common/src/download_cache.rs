use crate::hashing::{raw_hash, to_nix_base32, RawHash, WyHashBuilder};
use anyhow::{bail, Result};
use bincode::{Decode, Encode};
use std::{
    collections::HashMap,
    fmt::{Debug, Formatter},
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
};
use tokio::sync::{Mutex, OnceCell};
use tracing::{info, warn};

static CACHE: LazyLock<Mutex<HashMap<RawHash, Arc<OnceCell<Arc<[u8]>>>, WyHashBuilder>>> =
    LazyLock::new(|| Mutex::new(HashMap::default()));
static APPIMAGE_DIR: LazyLock<Option<PathBuf>> =
    LazyLock::new(|| std::env::var_os("MKWEBFONT_APPIMAGE_DATA").map(PathBuf::from));
static CACHE_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    let project_dirs = directories::ProjectDirs::from("moe.rimin", "", "mkwebfont")
        .expect("Could not get cache directory!");
    let mut cache_dir = project_dirs.cache_dir().to_path_buf();
    cache_dir.push("dl_cache");
    if !cache_dir.exists() {
        std::fs::create_dir_all(&cache_dir).expect("Could not create cache directory.");
    } else if !cache_dir.is_dir() {
        panic!("Cache directory error.");
    }
    cache_dir
});

#[derive(Clone, Encode, Decode)]
pub struct DownloadInfo {
    filename_prefix: String,
    filename_suffix: String,
    url: String,
    size: u64,
    hash: RawHash,
}
impl DownloadInfo {
    pub fn for_file(path: &Path, url: &str) -> Result<DownloadInfo> {
        let filename = url.split('/').last().expect("Bad path.");
        let (filename_prefix, filename_suffix) = if filename.contains(".") {
            let mut iter = filename.rsplitn(2, '.');
            let filename_suffix = iter.next().expect("Bad filename.");
            let filename_prefix = iter.next().expect("Bad filename.");
            (filename_prefix.to_string(), format!(".{filename_suffix}"))
        } else {
            (filename.to_string(), String::from(".dat"))
        };
        let data = std::fs::read(&path)?;
        Ok(DownloadInfo {
            filename_prefix,
            filename_suffix,
            url: url.to_string(),
            size: data.len() as u64,
            hash: raw_hash(&data),
        })
    }

    async fn raw_load(&self) -> Result<Arc<[u8]>> {
        let filename = format!(
            "{}.{}{}",
            self.filename_prefix,
            to_nix_base32(&self.hash),
            self.filename_suffix
        );

        if let Some(appimage_dir) = &*APPIMAGE_DIR {
            let mut appimage_dir = appimage_dir.to_path_buf();
            appimage_dir.push(&filename);
            if appimage_dir.exists() {
                let data = std::fs::read(&appimage_dir)?;
                if data.len() as u64 != self.size || raw_hash(&data) != self.hash {
                    bail!("Hash does not match data in `.AppImage`!? Check it isn't corrupted.")
                }
                return Ok(data.into());
            }
        }

        let mut cache_path = CACHE_DIR.to_path_buf();
        cache_path.push(&filename);

        if cache_path.exists() {
            if !cache_path.is_file() {
                bail!("Cache directory contains subdirectories!? Just giving up.");
            }
            let data = std::fs::read(&cache_path)?;
            if data.len() as u64 != self.size || raw_hash(&data) != self.hash {
                warn!("Corrupted cache file: {}", cache_path.display());
                std::fs::remove_file(&cache_path)?;
            } else {
                return Ok(data.into());
            }
        }

        info!("Downloading '{}'...", self.url);
        let req = ureq::get(&self.url).call()?;
        let mut file_data = Vec::new();
        req.into_reader()
            .take(self.size)
            .read_to_end(&mut file_data)?;

        let mut cache_tmp_path = cache_path.clone();
        cache_tmp_path.pop();
        cache_tmp_path.push(format!("{filename}.download-tmp-{}", std::process::id()));

        // Avoid ever running a bad file to the cache.
        // This should work even if multiple instances of mkwebfont are trying do this.
        std::fs::write(&cache_tmp_path, &file_data)?;
        std::fs::rename(&cache_tmp_path, &cache_path)?;

        Ok(file_data.into())
    }

    pub async fn load(&self) -> Result<Arc<[u8]>> {
        let arc = CACHE.lock().await.entry(self.hash).or_default().clone();
        let result = arc
            .get_or_try_init(|| async { self.raw_load().await })
            .await?;
        Ok(result.clone())
    }
}
impl Debug for DownloadInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DownloadInfo")
            .field("filename_prefix", &self.filename_prefix)
            .field("filename_suffix", &self.filename_suffix)
            .field("url", &self.url)
            .field("size", &self.size)
            .field("hash", &to_nix_base32(&self.hash))
            .finish()
    }
}
