use crate::{gather_css::parse_font_families, webroot::RelaWebroot, RewriteContext};
use anyhow::{bail, Result};
use lightningcss::{
    declaration::DeclarationBlock,
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
    traits::{ToCss, Zero},
    values::{angle::Angle, size::Size2D, url::Url},
};
use mkwebfont_common::paths::get_relative_from;
use mkwebfont_fontops::font_info::FontStyle;
use std::borrow::Cow;
use tracing::{debug, info};

const DEFAULT_LOC: Location = Location { source_index: 0, line: 0, column: 0 };
const DEFAULT_LOC_CSS: lightningcss::dependencies::Location =
    lightningcss::dependencies::Location { line: 0, column: 0 };

fn printer() -> PrinterOptions<'static> {
    let mut options = PrinterOptions::default();
    options.minify = false;
    options
}

fn generate_font_face_stylesheet<'a, 'b>(
    ctx: &RewriteContext,
    store_uri: &str,
) -> StyleSheet<'a, 'b> {
    let mut sheet = StyleSheet::new(vec![], CssRuleList(vec![]), ParserOptions::default());
    let store_prefix = if store_uri.is_empty() {
        String::new()
    } else {
        format!("{store_uri}/")
    };
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
                        url: format!("{store_prefix}{}", subset.woff2_file_name()).into(),
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

fn rewrite_properties_for_fallback(
    ctx: &RewriteContext,
    properties: &mut DeclarationBlock,
) -> bool {
    // We do NOT warn about unrecgonized CSS here, because that should be done in the `gather_css`
    // phase.

    let mut rewritten = false;
    for property in properties
        .declarations
        .iter_mut()
        .chain(properties.important_declarations.iter_mut())
    {
        match property {
            Property::FontFamily(family) => {
                let families = parse_font_families(&family);
                let init_len = family.len();
                family.retain(|x| matches!(x, FontFamily::FamilyName(_)));
                if init_len != family.len() {
                    rewritten = true;
                }
                if ctx.add_fallback.contains(&families) {
                    family.push(FontFamily::FamilyName(ctx.fallback_font_name.clone().into()));
                    rewritten = true;
                }
            }
            _ => {}
        }
    }
    rewritten
}

fn rewrite_for_fallback(ctx: &RewriteContext, css: &mut [CssRule]) -> bool {
    let mut rewritten = false;
    for rule in css {
        match rule {
            CssRule::Media(media_query) => {
                rewritten |= rewrite_for_fallback(ctx, &mut media_query.rules.0);
            }
            CssRule::Style(rule) => {
                rewritten |= rewrite_properties_for_fallback(ctx, &mut rule.declarations);
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

pub fn generate_font_css(ctx: &RewriteContext) -> Result<String> {
    let Some(store_uri) = &ctx.store_uri else {
        bail!("`--store_uri` is required for generating detached font CSS.")
    };
    let sheet = generate_font_face_stylesheet(ctx, &store_uri);
    Ok(sheet.to_css(printer())?.code)
}

pub fn rewrite_style_attr(ctx: &RewriteContext, style: &str) -> Result<Option<String>> {
    match DeclarationBlock::parse_string(style, ParserOptions::default()) {
        Ok(mut block) => {
            if rewrite_properties_for_fallback(ctx, &mut block) {
                Ok(Some(block.to_css_string(printer())?))
            } else {
                Ok(None)
            }
        }
        Err(_) => Ok(None),
    }
}

pub fn rewrite_style_tag(ctx: &RewriteContext, style: &str) -> Result<Option<String>> {
    let mut sheet =
        StyleSheet::parse(style, ParserOptions::default()).map_err(|x| x.into_owned())?;
    if rewrite_for_fallback(ctx, &mut sheet.rules.0) {
        Ok(Some(sheet.to_css(printer())?.code))
    } else {
        Ok(None)
    }
}

pub fn process_css_path(
    ctx: &RewriteContext,
    root: &RelaWebroot,
    append_fonts: bool,
) -> Result<()> {
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
