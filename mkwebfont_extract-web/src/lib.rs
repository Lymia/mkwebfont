mod apply_rules;
mod extract_text;
mod font_info;
mod gather_css;
mod utils;
mod webroot;

// TODO: Add support for `style="..."`.

mod consts {
    pub const CACHE_SIZE: u64 = 128;
}

mod api {
    use crate::{font_info::TextInfoBuilder, gather_css::CssCache, webroot::Webroot, TextInfo};
    use anyhow::Result;
    use arcstr::ArcStr;
    use mkwebfont_common::join_set::JoinSet;
    use std::{
        path::{Path, PathBuf},
        sync::Arc,
    };
    use tokio::sync::RwLock;
    use tracing::{info, info_span};

    #[derive(Debug, Clone)]
    pub struct TextExtractor(Arc<TextExtractorData>);
    #[derive(Debug)]
    struct TextExtractorData {
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

        pub async fn build(&self) -> TextInfo {
            self.0.builder.read().await.build()
        }
    }
    impl TextExtractorData {
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
            let _enter = span.enter();

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
    }
    impl Default for TextExtractor {
        fn default() -> Self {
            TextExtractor(Arc::new(TextExtractorData {
                builder: Arc::new(RwLock::new(TextInfoBuilder::default())),
                css_cache: CssCache::new(),
            }))
        }
    }
}

pub use api::TextExtractor;
pub use font_info::{FontStackInfo, TextInfo, TextSample};
