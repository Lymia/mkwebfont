mod css_ops;

use crate::FontStackInfo;
use crate::utils::inner_html;
use crate::webroot::{RelaWebroot, Webroot};
use anyhow::Result;
use arcstr::ArcStr;
use mkwebfont_common::character_set::CharacterSet;
use mkwebfont_common::hashing::{WyHashMap, WyHashSet};
use mkwebfont_common::join_set::JoinSet;
use mkwebfont_fontops::subsetter::WebfontInfo;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use tracing::{Instrument, warn};
use tsugiki::dom::NodeRef;
use tsugiki::iter::NodeIterator;
use tsugiki::parse_document;
use tsugiki::select::SelectorSet;

#[derive(Default, Debug, Clone)]
pub struct RewriteInfo {
    webroots: WyHashMap<Arc<Path>, WebrootRewriteInfo>,
}

#[derive(Default, Debug, Clone)]
struct WebrootRewriteInfo {
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

fn process_html_path(
    ctx: &RewriteContext,
    _font_stacks: &[FontStackInfo],
    root: &RelaWebroot,
) -> Result<()> {
    static SELECTOR: LazyLock<SelectorSet> =
        LazyLock::new(|| SelectorSet::compile("style,*[style]").unwrap());

    let document = parse_document(std::fs::read_to_string(&root.file_name())?);
    let mut modified = false;
    for elem in SELECTOR.filter(document.inclusive_descendants().elements()) {
        if elem.borrow().name.local.as_bytes() == b"style" {
            let text = inner_html(elem.as_node());
            if let Some(text) = css_ops::rewrite_style_tag(ctx, &text)? {
                elem.as_node().children().for_each(|x| x.detach());
                elem.as_node().append(NodeRef::new_text(text));
                modified = true;
            }
        }

        let mut lock = elem.borrow_mut();
        let attrs = &mut lock.attributes;
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
    targets: &WebrootRewriteInfo,
    font_stacks: &Arc<[FontStackInfo]>,
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
        let font_stacks = font_stacks.clone();
        let root = webroot.rela(&path)?;
        joins.spawn(async move { process_html_path(&ctx, &font_stacks, &root) }.in_current_span());
    }
    joins.join().await?;
    Ok(())
}

impl RewriteContext {
    pub fn generate_font_css(&self) -> Result<String> {
        css_ops::generate_font_css(self)
    }
}

pub async fn perform_rewrite(
    targets: &RewriteInfo,
    font_stacks: &Arc<[FontStackInfo]>,
    ctx: Arc<RewriteContext>,
) -> Result<()> {
    let mut joins = JoinSet::new();
    for (root, targets) in &targets.webroots {
        let targets = targets.clone();
        let font_stacks = font_stacks.clone();
        let webroot = Webroot::new(root.to_path_buf())?;
        let ctx = ctx.clone();
        joins.spawn(
            async move { perform_rewrite_for_root(&targets, &font_stacks, &webroot, ctx).await }
                .in_current_span(),
        );
    }
    joins.join().await?;
    Ok(())
}

pub fn find_css_for_rewrite(
    targets: &mut RewriteInfo,
    document: &ArcStr,
    root: &RelaWebroot,
    used_stacks: WyHashSet<Arc<[ArcStr]>>,
) -> Result<()> {
    static SELECTOR: LazyLock<SelectorSet> =
        LazyLock::new(|| SelectorSet::compile("style,link[rel~=stylesheet],*[style]").unwrap());

    let document = parse_document(document.as_str());

    let mut css_list = Vec::new();
    let mut css_list_fonts = Vec::new();

    let webroot_info = targets
        .webroots
        .entry(root.root().root().into())
        .or_default();

    for elem in SELECTOR.filter(document.inclusive_descendants().elements()) {
        let elem = elem.borrow();

        match elem.name.local.as_bytes() {
            b"style" => {
                webroot_info
                    .rewrite_html_style
                    .insert(root.file_name().clone());
            }
            b"link" => {
                let path = root.resolve(elem.attributes.get("href").unwrap())?;
                if elem
                    .attributes
                    .get("rel")
                    .unwrap()
                    .contains("mkwebfont-out")
                {
                    css_list_fonts.push(path);
                } else {
                    css_list.push(path);
                }
            }
            _ => {}
        }
        if elem.attributes.get("style").is_some() {
            webroot_info
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
        webroot_info.rewrite_css_path.insert(path.into());
    }
    for path in css_list_fonts {
        if webroot_info.rewrite_css_path.contains(path.as_path()) {
            warn!("Path {} is used for @font-face generation only on some pages.", path.display());
            warn!("This may have unpredictable results.");
        }
        let path: Arc<Path> = path.into();
        webroot_info.rewrite_css_path_fonts.insert(path.clone());
        webroot_info
            .used_stacks
            .entry(path)
            .or_default()
            .extend(used_stacks.iter().cloned());
    }

    Ok(())
}
