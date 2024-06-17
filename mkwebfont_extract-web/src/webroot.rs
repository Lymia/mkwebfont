use anyhow::{bail, Result};
use mkwebfont_common::hashing::WyHashBuilder;
use moka::future::{Cache, CacheBuilder};
use std::{
    io,
    path::{Path, PathBuf},
    sync::Arc,
};
use tracing::debug;

#[derive(Clone)]
pub struct Webroot(Arc<WebrootData>);
struct WebrootData {
    root: PathBuf,
    cache: Cache<PathBuf, Arc<str>, WyHashBuilder>,
}
impl Webroot {
    pub fn new(root: PathBuf) -> Result<Self> {
        Ok(Webroot(Arc::new(WebrootData {
            root: root.canonicalize()?.to_path_buf(),
            cache: CacheBuilder::new(512).build_with_hasher(Default::default()),
        })))
    }

    fn canonicalize(&self, rela_root: &Path, mut path: &str) -> Result<PathBuf> {
        let resolved_root = if path.starts_with("/") {
            while path.starts_with("/") {
                path = &path[1..];
            }
            &self.0.root
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

        if !resolved.starts_with(&self.0.root) {
            bail!("Resolved path '{resolved:?}' is not child of '{:?}'", self.0.root);
        }

        Ok(resolved)
    }

    async fn cache_read(&self, path: PathBuf) -> Result<Arc<str>> {
        Ok(self
            .0
            .cache
            .try_get_with::<_, io::Error>(path.clone(), async move {
                let path: Arc<str> = std::fs::read_to_string(path)?.into();
                Ok(path)
            })
            .await?)
    }

    pub async fn load(&self, rela_root: &Path, path: &str) -> Result<Arc<str>> {
        self.cache_read(self.canonicalize(rela_root, path)?).await
    }

    pub async fn rela(&self, rela_root: &Path) -> Result<RelaWebroot> {
        let mut new_root = self.0.root.to_path_buf();
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
