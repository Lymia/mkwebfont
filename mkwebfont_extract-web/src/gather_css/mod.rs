use crate::{utils, webroot::RelaWebroot};
use anyhow::Result;
use arcstr::ArcStr;
use scraper::{Html, Selector};
use std::sync::Arc;
use tracing::warn;

mod parse;

pub use parse::*;

const CSS_BASE_RULES: ArcStr = arcstr::literal!(include_str!("base_rules.css"));

enum CssSource {
    RelFile(String),
    Embedded(String),
}

fn extract_css_sources(document: &ArcStr) -> Result<Vec<CssSource>> {
    let document = Html::parse_document(&document);
    let mut data = Vec::new();
    for tag in document.select(&Selector::parse("style,link[rel~=stylesheet]").unwrap()) {
        match tag.value().name.local.as_bytes() {
            b"link" => match tag.value().attr("href") {
                Some(x) => data.push(CssSource::RelFile(x.to_string())),
                None => warn!("Tag does not contain href: {tag:?}"),
            },
            b"style" => data.push(CssSource::Embedded(utils::direct_text_children(&tag).into())),
            _ => unreachable!(),
        }
    }
    Ok(data)
}

async fn gather_all_css(
    document: &ArcStr,
    root: &RelaWebroot,
    inject: &[ArcStr],
) -> Result<Vec<(ArcStr, RelaWebroot)>> {
    let mut result = Vec::new();
    result.push((CSS_BASE_RULES, root.clone()));
    for css in inject {
        result.push((css.clone(), root.clone()));
    }
    for source in extract_css_sources(document)? {
        match source {
            CssSource::RelFile(path) => match root.load_rela(&path).await {
                Ok(data) => result.push(data),
                Err(e) => warn!("Could not load '{path}': {e:?}"),
            },
            CssSource::Embedded(tag) => result.push((tag.into(), root.clone())),
        }
    }
    Ok(result)
}

async fn process_rules(
    sources: &[(ArcStr, RelaWebroot)],
    css_cache: &CssCache,
) -> Result<Vec<Arc<RawCssRule>>> {
    let mut rules: Vec<Arc<RawCssRule>> = Vec::new();
    for (source, new_root) in sources {
        for rule in &*css_cache.get_css(source.clone(), new_root).await? {
            rules.push(rule.clone());
        }
    }
    rules.sort_by_key(|x| x.specificity);
    Ok(rules)
}

impl CssCache {
    pub async fn get_rules_from_document(
        &self,
        document: &ArcStr,
        root: &RelaWebroot,
        inject: &[ArcStr],
    ) -> Result<Vec<Arc<RawCssRule>>> {
        let sources = gather_all_css(document, root, inject).await?;
        process_rules(&sources, self).await
    }
}
