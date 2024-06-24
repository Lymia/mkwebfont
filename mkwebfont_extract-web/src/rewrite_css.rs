use crate::{font_info::StylesheetInfo, gather_css::parse_font_families, webroot::RelaWebroot};
use anyhow::Result;
use arcstr::ArcStr;
use lightningcss::{
    properties::{font::FontFamily, Property},
    rules::CssRule,
    stylesheet::StyleSheet,
};
use mkwebfont_common::hashing::WyHashBuilder;
use scraper::{selectable::Selectable, Html, Selector};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
};
use tracing::warn;

#[derive(Default)]
pub struct RewriteInfo {
    rewrite_html_style: HashSet<Arc<Path>, WyHashBuilder>,
    rewrite_css_path: HashSet<Arc<Path>, WyHashBuilder>,
    append_font_face: HashSet<Arc<Path>, WyHashBuilder>,
}

pub struct RewriteContext {
    pub fallback_font_name: String,
    pub add_fallback: HashSet<Arc<[ArcStr]>, WyHashBuilder>,
}

fn rewrite_for_fallback(ctx: &mut RewriteContext, css: &mut [CssRule]) -> bool {
    // We do NOT warn about unrecgonized CSS here, because that should be done in the `gather_css`
    // phase.

    let mut rewritten = false;
    for rule in css {
        match rule {
            CssRule::Media(media_query) => {
                rewritten |= rewrite_for_fallback(ctx, &mut media_query.rules.0);
            }
            CssRule::Style(rule) => {
                for property in &mut rule.declarations.declarations {
                    match property {
                        Property::FontFamily(family) => {
                            let families = parse_font_families(&family);
                            family.retain(|x| matches!(x, FontFamily::FamilyName(_)));
                            if ctx.add_fallback.contains(&families) {
                                family.push(FontFamily::FamilyName(
                                    ctx.fallback_font_name.clone().into(),
                                ));
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    rewritten
}

pub async fn perform_rewrite(info: &RewriteInfo, ctx: &RewriteContext) -> Result<()> {
    // TODO: Write this eventually.
    Ok(())
}

pub fn find_css_for_rewrite(
    info: &mut RewriteInfo,
    document: &ArcStr,
    root: &RelaWebroot,
) -> Result<()> {
    static SELECTOR: LazyLock<Selector> =
        LazyLock::new(|| Selector::parse("style,link[rel~=stylesheet],*[style]").unwrap());

    let document = Html::parse_document(&document);

    let mut css_list = Vec::new();
    let mut css_list_fonts = Vec::new();

    for elem in document.select(&SELECTOR) {
        match elem.value().name.local.as_bytes() {
            b"style" => {
                info.rewrite_html_style.insert(root.file_name().clone());
            }
            b"link" => {
                let path = root.resolve(elem.attr("href").unwrap())?;
                if elem.attr("rel").unwrap().contains("mkwebfont-out") {
                    css_list_fonts.push(path);
                } else {
                    css_list.push(path);
                }
            }
            _ => {}
        }
        if elem.attr("style").is_some() {
            info.rewrite_html_style.insert(root.file_name().clone());
        }
    }

    if css_list_fonts.is_empty() && !css_list.is_empty() {
        if css_list.iter().filter(|x| !x.exists()).count() == 1 {
            css_list_fonts.push(
                css_list.remove(
                    css_list
                        .iter()
                        .enumerate()
                        .find(|x| x.1.exists())
                        .unwrap()
                        .0,
                ),
            );
        } else {
            warn!("Arbitrary adding @font-face declarations to the first stylesheet linked.");
            warn!("This is probably not what you want.");
            warn!("Add `rel=\"mkwebfont-out\"` to a single stylesheet tag to fix this.");
            css_list_fonts.push(css_list.remove(0));
        }
    }

    for path in css_list {
        info.rewrite_css_path.insert(path.into());
    }
    for path in css_list_fonts {
        if info.rewrite_css_path.contains(path.as_path()) {
            warn!("Path {} is used for @font-face generation only on some pages.", path.display());
            warn!("This may have unpredictable results.");
        }
        info.append_font_face.insert(path.into());
    }

    Ok(())
}
