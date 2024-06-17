mod parse;

use crate::{
    gather_css::parse::RawCssRule,
    utils,
    webroot::{RelaWebroot, Webroot},
};
use anyhow::Result;
use lightningcss::stylesheet::StyleSheet;
use scraper::{Html, Selector};
use std::{borrow::Cow, collections::HashMap, path::Path, sync::Arc};
use tracing::warn;

const CSS_BASE_RULES: &str = "
    area, datalist, head, link, param, script, style, title {
        display: none;
    }
";

#[derive(Debug)]
pub enum CssSource {
    Static(&'static str),
    Owned(String),
    Shared(Arc<str>),
}
impl CssSource {
    pub fn as_str(&self) -> &str {
        match self {
            CssSource::Static(str) => *str,
            CssSource::Owned(str) => str.as_str(),
            CssSource::Shared(str) => &str,
        }
    }
}

async fn gather_all_css(
    document: &Html,
    root: &RelaWebroot,
    inject: &[Arc<str>],
) -> Result<Vec<CssSource>> {
    let mut result = Vec::new();
    result.push(CssSource::Static(CSS_BASE_RULES));
    for css in inject {
        result.push(CssSource::Shared(css.clone()));
    }
    for tag in document.select(&Selector::parse("style,link[rel~=stylesheet]").unwrap()) {
        match tag.value().name.local.as_bytes() {
            b"link" => match tag.value().attr("href") {
                Some(x) => match root.load(x).await {
                    Ok(data) => result.push(CssSource::Shared(data)),
                    Err(e) => warn!("Could not load '{x}': {e:?}"),
                },
                None => warn!("{}", root.name().display()),
            },
            b"style" => result.push(CssSource::Owned(utils::direct_text_children(&tag))),
            _ => unreachable!(),
        }
    }

    Ok(result)
}

async fn process_rules(sources: &[CssSource], root: &RelaWebroot) -> Result<Vec<Arc<RawCssRule>>> {
    let mut rules = Vec::new();
    for source in sources {
        rules.extend(parse::parse_css(source.as_str(), root).await?);
    }
    rules.sort_by_key(|x| x.specificity);
    Ok(rules)
}

pub async fn raw_rules(
    document: &Html,
    root: &RelaWebroot,
    inject: &[Arc<str>],
) -> Result<Vec<Arc<RawCssRule>>> {
    let sources = gather_all_css(document, root, inject).await?;
    process_rules(&sources, root).await
}
