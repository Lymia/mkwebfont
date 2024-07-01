use crate::consts::CACHE_SIZE;
use anyhow::{bail, Result};
use arcstr::ArcStr;
use mkwebfont_common::hashing::WyHashBuilder;
use moka::future::{Cache, CacheBuilder};
use std::{
    io,
    path::{Path, PathBuf},
    sync::Arc,
};
use tracing::{debug, Instrument};

#[derive(Clone)]
pub struct Webroot(Arc<WebrootData>);
struct WebrootData {
    root: PathBuf,
    cache: Cache<PathBuf, ArcStr, WyHashBuilder>,
}
impl Webroot {
    pub fn new(root: PathBuf) -> Result<Self> {
        Ok(Webroot(Arc::new(WebrootData {
            root: root.canonicalize()?.to_path_buf(),
            cache: CacheBuilder::new(CACHE_SIZE).build_with_hasher(Default::default()),
        })))
    }

    pub fn resolve(&self, rela_root: Option<&Path>, mut path: &str) -> Result<PathBuf> {
        if path.contains('?') {
            path = path.split('?').next().unwrap();
        }

        let resolved_root = if path.starts_with("/") || rela_root.is_none() {
            while path.starts_with("/") {
                path = &path[1..];
            }
            &self.0.root
        } else {
            rela_root.unwrap()
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

    async fn cache_read(&self, path: &Path) -> Result<ArcStr> {
        Ok(self
            .0
            .cache
            .try_get_with::<_, io::Error>(
                path.to_path_buf(),
                async { Ok(std::fs::read_to_string(path)?.into()) }.in_current_span(),
            )
            .await?)
    }

    pub fn root(&self) -> &Path {
        &self.0.root
    }

    pub async fn load(&self, rela_root: Option<&Path>, path: &str) -> Result<ArcStr> {
        self.cache_read(&self.resolve(rela_root, path)?).await
    }

    pub async fn load_rela(
        &self,
        rela_root: Option<&Path>,
        path: &str,
    ) -> Result<(ArcStr, RelaWebroot)> {
        let path = self.resolve(rela_root, path)?;
        Ok((self.cache_read(&path).await?, self.rela(&path)?))
    }

    pub async fn load_rela_raw(&self, path: &Path) -> Result<(ArcStr, RelaWebroot)> {
        let path = path.canonicalize()?;
        let rela = self.rela(&path)?;
        Ok((self.cache_read(&path).await?, rela))
    }

    pub fn rela(&self, rela_root: &Path) -> Result<RelaWebroot> {
        let mut new_root = self.0.root.to_path_buf();
        new_root.push(rela_root);
        let path = new_root.canonicalize()?;

        if !path.starts_with(&self.0.root) {
            bail!("Relative path '{path:?}' is not child of '{:?}'", self.0.root);
        }

        Ok(RelaWebroot {
            root: self.clone(),
            parent: path.parent().unwrap().to_path_buf().into(),
            file_name: path.into(),
        })
    }
}

#[derive(Clone)]
pub struct RelaWebroot {
    root: Webroot,
    parent: Arc<Path>,
    file_name: Arc<Path>,
}
impl RelaWebroot {
    pub fn resolve(&self, path: &str) -> Result<PathBuf> {
        self.root.resolve(Some(&self.parent), path)
    }

    pub async fn load(&self, path: &str) -> Result<ArcStr> {
        self.root.load(Some(&self.parent), path).await
    }

    pub async fn load_rela(&self, path: &str) -> Result<(ArcStr, RelaWebroot)> {
        self.root.load_rela(Some(&self.parent), path).await
    }

    pub fn root(&self) -> &Webroot {
        &self.root
    }

    pub fn rela_key(&self) -> &Arc<Path> {
        &self.parent
    }

    pub fn file_name(&self) -> &Arc<Path> {
        &self.file_name
    }
}
