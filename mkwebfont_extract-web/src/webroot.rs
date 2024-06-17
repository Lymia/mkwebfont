use anyhow::{bail, Result};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::RwLock;
use tracing::debug;

#[derive(Clone)]
pub struct Webroot(Arc<RwLock<WebrootData>>);
struct WebrootData {
    root: Arc<Path>,
    cache: HashMap<PathBuf, Arc<str>>,
}
impl Webroot {
    pub fn new(root: PathBuf) -> Result<Self> {
        Ok(Webroot(Arc::new(RwLock::new(WebrootData {
            root: root.canonicalize()?.into(),
            cache: Default::default(),
        }))))
    }

    async fn canonicalize(&self, rela_root: &Path, mut path: &str) -> Result<PathBuf> {
        let root = self.0.read().await.root.clone();
        let resolved_root = if path.starts_with("/") {
            while path.starts_with("/") {
                path = &path[1..];
            }
            &root
        } else {
            rela_root
        };
        let mut tmp = resolved_root.to_path_buf();
        tmp.push(path);

        let resolved = tmp.canonicalize()?;
        debug!(
            "Relative path: '{}' + '{path}' = '{}'",
            resolved_root.display(),
            resolved.display()
        );

        if !resolved.starts_with(&root) {
            bail!("Resolved path '{resolved:?}' is not child of '{:?}'", root);
        }

        Ok(resolved)
    }

    async fn cache_read(&self, path: &Path) -> Result<Arc<str>> {
        let mut lock = self.0.read().await;
        if let Some(cached) = lock.cache.get(path) {
            Ok(cached.clone())
        } else {
            drop(lock);
            let data: Arc<str> = String::from_utf8_lossy(&std::fs::read(path)?)
                .to_string()
                .into();

            let mut lock = self.0.write().await;
            lock.cache.insert(path.to_path_buf(), data.clone());
            drop(lock);

            Ok(data)
        }
    }

    pub async fn load(&self, rela_root: &Path, path: &str) -> Result<Arc<str>> {
        self.cache_read(&self.canonicalize(rela_root, path).await?)
            .await
    }

    pub async fn rela(&self, rela_root: &Path) -> Result<RelaWebroot> {
        let mut new_root = self.0.read().await.root.to_path_buf();
        new_root.push(rela_root);
        let path = new_root.canonicalize()?;

        Ok(RelaWebroot {
            root: self.clone(),
            parent: path.parent().unwrap().to_path_buf(),
            rela_root: path,
        })
    }
}

pub struct RelaWebroot {
    root: Webroot,
    parent: PathBuf,
    rela_root: PathBuf,
}
impl RelaWebroot {
    pub async fn load(&self, path: &str) -> Result<Arc<str>> {
        self.root.load(&self.parent, path).await
    }

    pub fn name(&self) -> &Path {
        &self.rela_root
    }
}
