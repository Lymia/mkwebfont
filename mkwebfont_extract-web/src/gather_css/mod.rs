mod parse;

use crate::{
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

pub async fn gather_all_css(
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
