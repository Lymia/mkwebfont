use crate::{
    gather_css::parse_font_families,
    webroot::{RelaWebroot, Webroot},
};
use anyhow::Result;
use arcstr::ArcStr;
use lightningcss::{
    printer::PrinterOptions,
    properties::{
        font::{AbsoluteFontWeight, FontFamily, FontWeight as CssFontWeight},
        Property,
    },
    rules::{
        font_face::{
            FontFaceProperty, FontFaceRule, FontFormat, FontStyle as CssFontStyle, Source,
            UnicodeRange, UrlSource,
        },
        CssRule, CssRuleList, Location,
    },
    stylesheet::{ParserOptions, StyleSheet},
    traits::Zero,
    values::{angle::Angle, size::Size2D, url::Url},
};
use mkwebfont_common::{hashing::WyHashBuilder, join_set::JoinSet, paths::get_relative_from};
use mkwebfont_fontops::{font_info::FontStyle, subsetter::WebfontInfo};
use scraper::{Html, Selector};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
};
use tracing::{debug, info, warn};

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

const DEFAULT_LOC: Location = Location { source_index: 0, line: 0, column: 0 };
const DEFAULT_LOC_CSS: lightningcss::dependencies::Location =
    lightningcss::dependencies::Location { line: 0, column: 0 };

fn printer() -> PrinterOptions<'static> {
    let mut options = PrinterOptions::default();
    options.minify = true;
    options
}

pub fn generate_font_face_stylesheet<'a, 'b>(
    ctx: &RewriteContext,
    store_uri: &str,
) -> StyleSheet<'a, 'b> {
    let mut sheet = StyleSheet::new(vec![], CssRuleList(vec![]), ParserOptions::default());
    for font in &ctx.webfonts {
        let weight_range = font.weight_range();
        let weight_low = *weight_range.start();
        let weight_high = *weight_range.end();
        let weight_range = Size2D(
            CssFontWeight::Absolute(AbsoluteFontWeight::Weight(weight_low as f32)),
            CssFontWeight::Absolute(AbsoluteFontWeight::Weight(weight_high as f32)),
        );
        for subset in font.subsets() {
            let mut font_face = FontFaceRule { properties: vec![], loc: DEFAULT_LOC };
            font_face
                .properties
                .push(FontFaceProperty::FontFamily(FontFamily::FamilyName(
                    font.font_family().to_string().into(),
                )));
            font_face.properties.push(FontFaceProperty::FontStyle(
                match font.parsed_font_style() {
                    FontStyle::Regular => CssFontStyle::Normal,
                    FontStyle::Italic => CssFontStyle::Italic,
                    FontStyle::Oblique => {
                        // TODO: Figure out how to grab the proper Oblique angle
                        CssFontStyle::Oblique(Size2D(Angle::zero(), Angle::zero()))
                    }
                },
            ));
            font_face
                .properties
                .push(FontFaceProperty::FontWeight(weight_range.clone()));
            font_face.properties.push(FontFaceProperty::UnicodeRange(
                subset
                    .unicode_ranges()
                    .into_iter()
                    .map(|r| UnicodeRange { start: *r.start(), end: *r.end() })
                    .collect(),
            ));
            font_face
                .properties
                .push(FontFaceProperty::Source(vec![Source::Url(UrlSource {
                    url: Url {
                        url: format!("{store_uri}/{}", subset.woff2_file_name()).into(),
                        loc: DEFAULT_LOC_CSS,
                    },
                    format: Some(FontFormat::WOFF2),
                    tech: vec![],
                })]));
            sheet.rules.0.push(CssRule::FontFace(font_face));
        }
    }
    sheet
}

fn rewrite_for_fallback(ctx: &RewriteContext, css: &mut [CssRule]) -> bool {
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

fn add_font_faces(css: &mut StyleSheet, ctx: &RewriteContext, store_url: &str) {
    let sheet = generate_font_face_stylesheet(ctx, store_url);
    css.rules.0.extend(sheet.rules.0);
}

fn find_store_uri<'a>(ctx: &'a RewriteContext, root: &RelaWebroot) -> Result<Cow<'a, str>> {
    if let Some(uri) = &ctx.store_uri {
        Ok(Cow::Borrowed(uri.as_str()))
    } else {
        Ok(Cow::Owned(get_relative_from(&root.file_name(), &ctx.store_path)?))
    }
}

fn rewrite_css(ctx: &RewriteContext, root: &RelaWebroot, append_fonts: bool) -> Result<()> {
    let data = std::fs::read_to_string(root.file_name())?;
    let mut sheet =
        StyleSheet::parse(&data, ParserOptions::default()).map_err(|x| x.into_owned())?;
    let mut rewritten = rewrite_for_fallback(ctx, &mut sheet.rules.0);
    if append_fonts {
        let store_uri = if let Some(uri) = &ctx.store_uri {
            Cow::Borrowed(uri.as_str())
        } else {
            Cow::Owned(get_relative_from(&root.file_name(), &ctx.store_path)?)
        };
        debug!(
            "(Appending fonts) Store URI for {} -> {}: {store_uri}",
            root.file_name().display(),
            ctx.store_path.display(),
        );
        add_font_faces(&mut sheet, ctx, &find_store_uri(ctx, root)?);
        rewritten = true;
    }
    if rewritten {
        info!("Writing modified CSS to {}...", root.file_name().display());
        std::fs::write(root.file_name(), sheet.to_css(printer())?.code)?;
    } else {
        debug!("CSS does not need rewriting.");
    }
    Ok(())
}

fn generate_css(ctx: &RewriteContext, root: &RelaWebroot) -> Result<()> {
    let sheet = generate_font_face_stylesheet(ctx, &find_store_uri(ctx, root)?);
    info!("Writing @font-face CSS to {}...", root.file_name().display());
    std::fs::write(root.file_name(), sheet.to_css(printer())?.code)?;
    Ok(())
}

fn process_css_path(ctx: &RewriteContext, root: &RelaWebroot, append_fonts: bool) -> Result<()> {
    if !root.file_name().exists() {
        if !append_fonts {
            // Warned about in gather_css
            Ok(())
        } else {
            generate_css(ctx, root)
        }
    } else {
        rewrite_css(ctx, root, append_fonts)
    }
}

fn process_html_path(_ctx: &RewriteContext, root: &RelaWebroot) -> Result<()> {
    warn!("TODO: HTML rewriting is not yet implemented: {}", root.file_name().display());
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
        joins.spawn(async move { process_css_path(&ctx, &root, append_fonts) });
    }
    for path in &targets.rewrite_html_style {
        let ctx = ctx.clone();
        let root = webroot.rela(&path)?;
        joins.spawn(async move { process_html_path(&ctx, &root) });
    }
    joins.join().await?;
    Ok(())
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
