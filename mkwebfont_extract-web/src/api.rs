use crate::{
    gather_css::CssCache,
    rewrite_css::{RewriteContext, RewriteTargets},
    webroot::Webroot,
    webroot_info::TextInfoBuilder,
    WebrootInfo,
};
use anyhow::Result;
use arcstr::ArcStr;
use mkwebfont_common::join_set::JoinSet;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::RwLock;
use tracing::{info, info_span, Instrument};

#[derive(Debug, Clone)]
pub struct WebrootInfoExtractor(Arc<WebrootInfoExtractorData>);
#[derive(Debug)]
struct WebrootInfoExtractorData {
    builder: Arc<RwLock<TextInfoBuilder>>,
    target: Arc<RwLock<RewriteTargets>>,
    css_cache: CssCache,
}
impl WebrootInfoExtractor {
    pub fn new() -> Self {
        Default::default()
    }

    fn convert_inject_css(inject_css: &[&str]) -> Vec<ArcStr> {
        inject_css.iter().map(|x| ArcStr::from(*x)).collect()
    }

    pub async fn push_document(&self, path: &Path, inject_css: &[&str]) -> Result<()> {
        let webroot = Webroot::new(PathBuf::from("/"))?;
        self.0
            .push_rules(&webroot, path, &Self::convert_inject_css(inject_css))
            .await?;
        Ok(())
    }

    pub async fn push_webroot(&self, path: &Path, inject_css: &[&str]) -> Result<()> {
        info!("Processing webroot at '{}'...", path.display());

        let webroot = Webroot::new(PathBuf::from(path))?;
        let inject_css = Arc::new(Self::convert_inject_css(inject_css));

        let mut joins = JoinSet::new();
        for path in glob::glob(&format!("{}/**/*.html", path.display()))? {
            let data = self.0.clone();
            let webroot = webroot.clone();
            let path = path?;
            let inject_css = inject_css.clone();
            joins.spawn(async move {
                data.push_rules(&webroot, &path, &inject_css).await?;
                Ok(())
            });
        }
        let count = joins.join().await?.len();

        info!("Processed {count} pages from '{}'!", path.display());

        Ok(())
    }

    pub async fn build(&self) -> WebrootInfo {
        self.0
            .builder
            .read()
            .await
            .build(&(*self.0.target.read().await))
    }
}
impl WebrootInfoExtractorData {
    async fn push_rules(
        &self,
        webroot: &Webroot,
        target: &Path,
        inject_css: &[ArcStr],
    ) -> Result<()> {
        info!("Processing HTML from '{}'...", target.display());

        let file_name = match target.file_name() {
            None => "<unknown>".to_string(),
            Some(target) => target.to_string_lossy().to_string(),
        };

        let span = info_span!("parse_html", name = file_name);
        async {
            let (data, root) = webroot.load_rela_raw(target).await?;
            crate::extract_text::extract_text(
                &data,
                &root,
                &self.css_cache,
                inject_css,
                self.builder.clone(),
            )
            .await?;
            {
                let mut write = self.target.write().await;
                crate::rewrite_css::find_css_for_rewrite(&mut write, &data, &root)?;
            }

            Ok(())
        }
        .instrument(span)
        .await
    }
}
impl Default for WebrootInfoExtractor {
    fn default() -> Self {
        WebrootInfoExtractor(Arc::new(WebrootInfoExtractorData {
            builder: Arc::new(RwLock::new(TextInfoBuilder::default())),
            target: Arc::new(RwLock::new(RewriteTargets::default())),
            css_cache: CssCache::new(),
        }))
    }
}

impl WebrootInfo {
    pub async fn rewrite_webroot(&self, ctx: RewriteContext) -> Result<()> {
        crate::rewrite_css::perform_rewrite(&self.targets, Arc::new(ctx)).await?;
        Ok(())
    }
}
