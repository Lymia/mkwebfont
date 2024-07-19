mod css_ops;

use crate::{
    utils::inner_html,
    webroot::{RelaWebroot, Webroot},
};
use anyhow::Result;
use arcstr::ArcStr;
use kuchikiki::{iter::NodeIterator, parse_html, traits::TendrilSink, NodeRef, Selectors};
use mkwebfont_common::{
    character_set::CharacterSet,
    hashing::{WyHashMap, WyHashSet},
    join_set::JoinSet,
};
use mkwebfont_fontops::subsetter::WebfontInfo;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
};
use tracing::{warn, Instrument};

#[derive(Default, Debug, Clone)]
pub struct RewriteTargets {
    targets: WyHashMap<Arc<Path>, WebrootRewriteTargets>,
}

#[derive(Default, Debug, Clone)]
struct WebrootRewriteTargets {
    rewrite_html_style: WyHashSet<Arc<Path>>,
    rewrite_css_path: WyHashSet<Arc<Path>>,
    rewrite_css_path_fonts: WyHashSet<Arc<Path>>,
    used_stacks: WyHashMap<Arc<Path>, WyHashSet<Arc<[ArcStr]>>>,
}

#[derive(Debug, Default, Clone)]
pub struct RewriteContext {
    pub fallback_font_name: String,
    pub fallback_info: WyHashMap<Arc<[ArcStr]>, CharacterSet>,
    pub webfonts: Vec<Arc<WebfontInfo>>,
    pub store_path: PathBuf,
    pub store_uri: Option<String>,
}

fn process_html_path(ctx: &RewriteContext, root: &RelaWebroot) -> Result<()> {
    static SELECTOR: LazyLock<Selectors> =
        LazyLock::new(|| Selectors::compile("style,*[style]").unwrap());

    let document = parse_html().one(std::fs::read_to_string(&root.file_name())?);
    let mut modified = false;
    for elem in SELECTOR.filter(document.inclusive_descendants().elements()) {
        if elem.name.local.as_bytes() == b"style" {
            let text = inner_html(elem.as_node());
            if let Some(text) = css_ops::rewrite_style_tag(ctx, &text)? {
                elem.as_node().children().for_each(|x| x.detach());
                elem.as_node().append(NodeRef::new_text(text));
                modified = true;
            }
        }

        let mut attrs = elem.attributes.borrow_mut();
        if let Some(text) = attrs.get("style") {
            if let Some(text) = css_ops::rewrite_style_attr(ctx, text)? {
                attrs.insert("style", text);
                modified = true;
            }
        }
    }

    if modified {
        document.serialize_to_file(root.file_name())?;
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

        let used_stacks = targets.used_stacks.get(path).cloned();
        joins.spawn(
            async move { css_ops::process_css_path(&ctx, &root, append_fonts, used_stacks.as_ref()) }
                .in_current_span(),
        );
    }
    for path in &targets.rewrite_html_style {
        let ctx = ctx.clone();
        let root = webroot.rela(&path)?;
        joins.spawn(async move { process_html_path(&ctx, &root) }.in_current_span());
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
        joins.spawn(
            async move { perform_rewrite_for_root(&targets, &webroot, ctx).await }
                .in_current_span(),
        );
    }
    joins.join().await?;
    Ok(())
}

pub fn find_css_for_rewrite(
    targets: &mut RewriteTargets,
    document: &ArcStr,
    root: &RelaWebroot,
    used_stacks: WyHashSet<Arc<[ArcStr]>>,
) -> Result<()> {
    static SELECTOR: LazyLock<Selectors> =
        LazyLock::new(|| Selectors::compile("style,link[rel~=stylesheet],*[style]").unwrap());

    let document = parse_html().one(document.as_str());

    let mut css_list = Vec::new();
    let mut css_list_fonts = Vec::new();

    let root_target = targets
        .targets
        .entry(root.root().root().into())
        .or_default();

    for elem in SELECTOR.filter(document.inclusive_descendants().elements()) {
        match elem.name.local.as_bytes() {
            b"style" => {
                root_target
                    .rewrite_html_style
                    .insert(root.file_name().clone());
            }
            b"link" => {
                let attrs = elem.attributes.borrow();
                let path = root.resolve(attrs.get("href").unwrap())?;
                if attrs.get("rel").unwrap().contains("mkwebfont-out") {
                    css_list_fonts.push(path);
                } else {
                    css_list.push(path);
                }
            }
            _ => {}
        }
        if elem.attributes.borrow().get("style").is_some() {
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
        } else if css_list.len() == 1 {
            css_list_fonts.extend(css_list.drain(..));
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
        let path: Arc<Path> = path.into();
        root_target.rewrite_css_path_fonts.insert(path.clone());
        root_target
            .used_stacks
            .entry(path)
            .or_default()
            .extend(used_stacks.iter().cloned());
    }

    Ok(())
}
