mod apply_rules;
mod extract_text;
mod font_info;
mod gather_css;
mod utils;
mod webroot;

mod consts {
    pub const CACHE_SIZE: u64 = 128;
}

mod api {
    use crate::{font_info::TextInfoBuilder, gather_css::CssCache, webroot::Webroot, TextInfo};
    use anyhow::Result;
    use arcstr::ArcStr;
    use std::{
        path::{Path, PathBuf},
        sync::Arc,
    };
    use tokio::sync::RwLock;
    use tracing::info;

    #[derive(Debug)]
    pub struct TextExtractor {
        builder: Arc<RwLock<TextInfoBuilder>>,
        css_cache: CssCache,
    }
    impl TextExtractor {
        pub fn new() -> Self {
            Default::default()
        }

        fn convert_inject_css(inject_css: &[&str]) -> Vec<ArcStr> {
            inject_css.iter().map(|x| ArcStr::from(*x)).collect()
        }
        async fn push_rules(
            &self,
            webroot: &Webroot,
            target: &Path,
            inject_css: &[ArcStr],
        ) -> Result<()> {
            info!("Processing HTML from '{}'...", target.display());

            let (data, root) = webroot.load_rela_raw(target).await?;
            crate::extract_text::extract_text(
                &data,
                &root,
                &self.css_cache,
                inject_css,
                self.builder.clone(),
            )
            .await?;
            Ok(())
        }

        pub async fn push_document(&self, path: &Path, inject_css: &[&str]) -> Result<()> {
            let webroot = Webroot::new(PathBuf::from("/"))?;
            self.push_rules(&webroot, path, &Self::convert_inject_css(inject_css))
                .await?;
            Ok(())
        }

        pub async fn push_webroot(&self, path: &Path, inject_css: &[&str]) -> Result<()> {
            let webroot = Webroot::new(PathBuf::from(path))?;
            let inject_css = Self::convert_inject_css(inject_css);

            for path in glob::glob(&format!("{}/**/*.html", path.display()))? {
                let path = path?;
                self.push_rules(&webroot, &path, &inject_css).await?;
            }

            Ok(())
        }

        pub async fn build(&self) -> TextInfo {
            self.builder.read().await.build()
        }
    }
    impl Default for TextExtractor {
        fn default() -> Self {
            TextExtractor {
                builder: Arc::new(RwLock::new(TextInfoBuilder::default())),
                css_cache: CssCache::new(),
            }
        }
    }
}

pub use api::TextExtractor;
pub use font_info::{FontStackInfo, TextInfo, TextSample};
