mod css_ops;

use crate::webroot::{RelaWebroot, Webroot};
use anyhow::Result;
use arcstr::ArcStr;
use html5ever::{
    interface::{ElementFlags, TreeSink},
    tree_builder::NodeOrText,
    Attribute,
};
use mkwebfont_common::{hashing::WyHashBuilder, join_set::JoinSet};
use mkwebfont_fontops::subsetter::WebfontInfo;
use scraper::{Html, Selector};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
};
use tracing::warn;

#[derive(Default, Debug, Clone)]
pub struct RewriteTargets {
    targets: HashMap<Arc<Path>, WebrootRewriteTargets, WyHashBuilder>,
}

#[derive(Default, Debug, Clone)]
struct WebrootRewriteTargets {
    rewrite_html_style: HashSet<Arc<Path>, WyHashBuilder>,
    rewrite_css_path: HashSet<Arc<Path>, WyHashBuilder>,
    rewrite_css_path_fonts: HashSet<Arc<Path>, WyHashBuilder>,
}

#[derive(Debug)]
pub struct RewriteContext {
    pub fallback_font_name: String,
    pub add_fallback: HashSet<Arc<[ArcStr]>, WyHashBuilder>,
    pub webfonts: Vec<WebfontInfo>,
    pub store_path: PathBuf,
    pub store_uri: Option<String>,
}

fn process_html_path(ctx: &RewriteContext, root: &RelaWebroot) -> Result<()> {
    static SELECTOR: LazyLock<Selector> =
        LazyLock::new(|| Selector::parse("style,*[style]").unwrap());

    let mut document = Html::parse_document(&std::fs::read_to_string(&root.file_name())?);
    let mut remove_nodes = Vec::new();
    let mut append_text = Vec::new();
    let mut change_style_tag = Vec::new();
    for elem in document.select(&SELECTOR) {
        if elem.value().name.local.as_bytes() == b"style" {
            let text = elem.inner_html();
            if let Some(text) = css_ops::rewrite_style_tag(ctx, &text)? {
                remove_nodes.extend(elem.children().map(|x| x.id()));
                append_text.push((elem.id(), text));
            }
        }
        if let Some(text) = elem.attr("style") {
            if let Some(text) = css_ops::rewrite_style_attr(ctx, text)? {
                change_style_tag.push((elem.id(), elem.value().clone(), text));
            }
        }
    }

    let mut modified = false;
    for node in remove_nodes {
        document.remove_from_parent(&node);
        modified = true;
    }
    for (node, text) in append_text {
        document.append(&node, NodeOrText::AppendText(text.into()));
        modified = true;
    }
    for (old, value, text) in change_style_tag {
        let mut attributes = Vec::new();
        for (name, value) in value.attrs {
            if name.local.as_bytes() != b"style" {
                attributes.push(Attribute { name, value });
            } else {
                attributes.push(Attribute { name, value: text.clone().into() })
            }
        }
        let new = document.create_element(value.name, attributes, ElementFlags::default());
        document.append_before_sibling(&old, NodeOrText::AppendNode(new));
        document.reparent_children(&old, &new);
        document.remove_from_parent(&old);
        modified = true;
    }

    if modified {
        std::fs::write(root.file_name(), document.html())?;
    }

    Ok(())
}

async fn perform_rewrite_for_root(
    targets: &WebrootRewriteTargets,
    webroot: &Webroot,
    ctx: Arc<RewriteContext>,
) -> Result<()> {
    let mut joins = JoinSet::new();
    for (path, append_fonts) in targets
        .rewrite_css_path
        .iter()
        .map(|x| (x, false))
        .chain(targets.rewrite_css_path_fonts.iter().map(|x| (x, true)))
    {
        let ctx = ctx.clone();
        let root = webroot.rela(&path)?;
        joins.spawn(async move { css_ops::process_css_path(&ctx, &root, append_fonts) });
    }
    for path in &targets.rewrite_html_style {
        let ctx = ctx.clone();
        let root = webroot.rela(&path)?;
        joins.spawn(async move { process_html_path(&ctx, &root) });
    }
    joins.join().await?;
    Ok(())
}

impl RewriteContext {
    pub fn generate_font_css(&self) -> Result<String> {
        css_ops::generate_font_css(self)
    }
}

pub async fn perform_rewrite(targets: &RewriteTargets, ctx: Arc<RewriteContext>) -> Result<()> {
    let mut joins = JoinSet::new();
    for (root, targets) in &targets.targets {
        let targets = targets.clone();
        let webroot = Webroot::new(root.to_path_buf())?;
        let ctx = ctx.clone();
        joins.spawn(async move { perform_rewrite_for_root(&targets, &webroot, ctx).await });
    }
    joins.join().await?;
    Ok(())
}

pub fn find_css_for_rewrite(
    targets: &mut RewriteTargets,
    document: &ArcStr,
    root: &RelaWebroot,
) -> Result<()> {
    static SELECTOR: LazyLock<Selector> =
        LazyLock::new(|| Selector::parse("style,link[rel~=stylesheet],*[style]").unwrap());

    let document = Html::parse_document(&document);

    let mut css_list = Vec::new();
    let mut css_list_fonts = Vec::new();

    let root_target = targets
        .targets
        .entry(root.root().root().into())
        .or_default();

    for elem in document.select(&SELECTOR) {
        match elem.value().name.local.as_bytes() {
            b"style" => {
                root_target
                    .rewrite_html_style
                    .insert(root.file_name().clone());
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
            root_target
                .rewrite_html_style
                .insert(root.file_name().clone());
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
        root_target.rewrite_css_path.insert(path.into());
    }
    for path in css_list_fonts {
        if root_target.rewrite_css_path.contains(path.as_path()) {
            warn!("Path {} is used for @font-face generation only on some pages.", path.display());
            warn!("This may have unpredictable results.");
        }
        root_target.rewrite_css_path_fonts.insert(path.into());
    }

    Ok(())
}
