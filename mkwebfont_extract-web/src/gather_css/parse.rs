use crate::{consts::CACHE_SIZE, webroot::RelaWebroot};
use anyhow::{bail, ensure, Error, Result};
use arcstr::ArcStr;
use async_recursion::async_recursion;
use cssparser::ToCss as CssParserToString;
use kuchikiki::Selectors;
use lightningcss::{
    declaration::DeclarationBlock,
    printer::PrinterOptions,
    properties::{
        custom::{CustomProperty, CustomPropertyName, Token, TokenOrValue, UnparsedProperty},
        display::{Display, DisplayKeyword},
        font::{AbsoluteFontWeight, FontFamily, FontStyle, FontWeight, GenericFontFamily},
        Property, PropertyId,
    },
    rules::{style::StyleRule, CssRule, CssRuleList},
    selector::Component,
    stylesheet::{ParserOptions, StyleSheet},
    traits::ToCss,
};
use mkwebfont_common::hashing::WyHashBuilder;
use moka::future::{Cache, CacheBuilder};
use std::{borrow::Cow, path::Path, sync::Arc};
use tracing::{info, info_span, warn, Instrument};

#[derive(Clone, Debug)]
pub struct RawCssRule {
    pub selector: Arc<Selectors>,
    pub is_conditional: bool,
    pub pseudo_element: Option<ArcStr>,
    pub declarations: Arc<RawCssRuleDeclarations>,
    pub specificity: u32,
}

#[derive(Clone, Debug)]
pub enum ParsedCssRule<T> {
    Override(T),
    OverrideUnset,
    Inherit,
    NoneSet,
    IgnoreSet,
}
impl<T> ParsedCssRule<T> {
    pub fn map<R>(&self, func: impl FnOnce(&T) -> R) -> ParsedCssRule<R> {
        match self {
            ParsedCssRule::Override(v) => ParsedCssRule::Override(func(v)),
            ParsedCssRule::OverrideUnset => ParsedCssRule::OverrideUnset,
            ParsedCssRule::Inherit => ParsedCssRule::Inherit,
            ParsedCssRule::NoneSet => ParsedCssRule::NoneSet,
            ParsedCssRule::IgnoreSet => ParsedCssRule::IgnoreSet,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RawCssRuleDeclarations {
    pub font_stack: ParsedCssRule<Arc<[ArcStr]>>,
    pub font_weight: ParsedCssRule<AbsoluteFontWeight>,
    pub font_style: ParsedCssRule<FontStyle>,
    pub is_displayed: ParsedCssRule<bool>,
    pub content: ParsedCssRule<ArcStr>,
}

/// Parses CSS font families into the form used in the rest of this subcrate.
pub fn parse_font_families(families: &[FontFamily<'_>]) -> ParsedCssRule<Arc<[ArcStr]>> {
    let mut new = Vec::new();
    for family in families {
        match family {
            FontFamily::Generic(GenericFontFamily::Inherit) => return ParsedCssRule::Inherit,
            FontFamily::Generic(_) => {
                warn!("Generic font families are ignored: {family:?}")
            }
            FontFamily::FamilyName(name) => new.push(name.to_lowercase().into()),
        }
    }
    if new.is_empty() {
        warn!("Found empty fonts list (excluding generics)");
        ParsedCssRule::IgnoreSet
    } else {
        ParsedCssRule::Override(new.into())
    }
}

/// Parses the list of declarations in a CSS rule into only the ones we need.
pub fn parse_declarations(style: &DeclarationBlock) -> Result<Option<RawCssRuleDeclarations>> {
    let mut raw_declarations = RawCssRuleDeclarations {
        font_stack: ParsedCssRule::NoneSet,
        font_weight: ParsedCssRule::NoneSet,
        font_style: ParsedCssRule::NoneSet,
        is_displayed: ParsedCssRule::NoneSet,
        content: ParsedCssRule::NoneSet,
    };
    let mut is_interesting = false;

    if !style.important_declarations.is_empty() {
        warn!("`!important` is not handled correctly.");
    }

    for declaration in style
        .important_declarations
        .iter()
        .chain(style.declarations.iter())
    {
        /// Parses CSS font weight declarations.
        fn parse_font_weight(weight: &FontWeight) -> ParsedCssRule<AbsoluteFontWeight> {
            match weight {
                FontWeight::Absolute(v) => ParsedCssRule::Override(v.clone()),
                FontWeight::Bolder | FontWeight::Lighter => {
                    warn!("Relative font weights are not supported.");
                    ParsedCssRule::NoneSet
                }
            }
        }

        match declaration {
            Property::Display(kind) => {
                if let Display::Keyword(DisplayKeyword::None) = kind {
                    raw_declarations.is_displayed = ParsedCssRule::Override(false);
                } else {
                    raw_declarations.is_displayed = ParsedCssRule::Override(true);
                }
                is_interesting = true;
            }

            Property::Font(font) => {
                raw_declarations.font_stack = parse_font_families(&font.family);
                raw_declarations.font_weight = parse_font_weight(&font.weight);
                raw_declarations.font_style = ParsedCssRule::Override(font.style.clone());
                is_interesting = true;
            }
            Property::FontFamily(family) => {
                raw_declarations.font_stack = parse_font_families(&family);
                is_interesting = true;
            }
            Property::FontWeight(weight) => {
                raw_declarations.font_weight = parse_font_weight(weight);
                is_interesting = true;
            }
            Property::FontStyle(style) => {
                raw_declarations.font_style = ParsedCssRule::Override(style.clone());
                is_interesting = true;
            }

            // Custom properties parsing
            Property::Unparsed(UnparsedProperty { property_id, value })
                if value.0.len() == 1
                    && match &value.0[0] {
                        TokenOrValue::Token(Token::Ident(id)) if *id == "inherit" => true,
                        _ => false,
                    } =>
            {
                match property_id {
                    PropertyId::Display => {
                        raw_declarations.is_displayed = ParsedCssRule::Inherit;
                        is_interesting = true;
                    }
                    PropertyId::Font => {
                        raw_declarations.font_stack = ParsedCssRule::Inherit;
                        raw_declarations.font_weight = ParsedCssRule::Inherit;
                        raw_declarations.font_style = ParsedCssRule::Inherit;
                        is_interesting = true;
                    }
                    PropertyId::FontFamily => {
                        raw_declarations.font_stack = ParsedCssRule::Inherit;
                        is_interesting = true;
                    }
                    PropertyId::FontWeight => {
                        raw_declarations.font_weight = ParsedCssRule::Inherit;
                        is_interesting = true;
                    }
                    PropertyId::FontStyle => {
                        raw_declarations.font_style = ParsedCssRule::Inherit;
                        is_interesting = true;
                    }
                    _ => {}
                }
            }
            Property::Unparsed(UnparsedProperty { property_id, value }) => match property_id {
                PropertyId::Display => warn!("Unparsed display property: {value:?}"),
                PropertyId::Font => warn!("Unparsed font property: {value:?}"),
                PropertyId::FontFamily => warn!("Unparsed font-family property: {value:?}"),
                PropertyId::FontWeight => warn!("Unparsed font-weight property: {value:?}"),
                PropertyId::FontStyle => warn!("Unparsed font-style property: {value:?}"),
                _ => {}
            },
            Property::Custom(CustomProperty { name: CustomPropertyName::Unknown(name), value }) => {
                match name.0.as_ref() {
                    "font" => {
                        warn!("Unparsed font");
                    }
                    "font-family" => {
                        warn!("Unparsed font-family");
                    }
                    "font-weight" => {
                        warn!("Unparsed font-weight");
                    }
                    "font-style" => {
                        warn!("Unparsed font-style");
                    }
                    "content" => {
                        if value.0.len() == 1 {
                            match &value.0[0] {
                                TokenOrValue::Token(Token::String(str)) => {
                                    raw_declarations.content =
                                        ParsedCssRule::Override(str.to_string().into());
                                    is_interesting = true;
                                }
                                TokenOrValue::Token(Token::Ident(id)) if *id == "none" => {
                                    raw_declarations.content = ParsedCssRule::OverrideUnset;
                                    is_interesting = true;
                                }
                                _ => warn!("Could not parse `content` attribute: {value:?}"),
                            }
                        } else {
                            warn!("Could not parse `content` attribute: {value:?}");
                        }
                    }
                    // TODO: Support stylistic sets and font variation settings.
                    _ => {}
                }
            }

            // ignore all other properties
            _ => {}
        }
    }

    if is_interesting {
        Ok(Some(raw_declarations))
    } else {
        Ok(None)
    }
}

/// Parses CSS data into a list of CSS rules.
async fn parse_css(
    data: &str,
    root: &RelaWebroot,
    cache: &CssCache,
) -> Result<Vec<Arc<RawCssRule>>> {
    /// The result of filtering a selector.
    #[derive(Debug)]
    struct FilteredSelector<'a> {
        selector: lightningcss::selector::Selector<'a>,
        is_conditional: bool,
        pseudo_element: Option<String>,
        specificity: u32,
    }

    /// Filters a selector.
    fn filter_selector<'a>(
        root_selector: &lightningcss::selector::Selector<'a>,
        selector: &lightningcss::selector::Selector<'a>,
    ) -> Result<FilteredSelector<'a>> {
        let mut components = Vec::new();
        let mut conditional = false;
        let mut pseudo_element = None;

        let mut combinator_early = Vec::new();
        let mut combinator_late = Vec::new();

        for component in selector.iter_raw_parse_order_from(0) {
            match component {
                // Unsupported by `scrapers`.
                Component::Scope => {
                    bail!("`:scope` is not supported: {root_selector:?}");
                }
                Component::Slotted(_)
                | Component::Part(_)
                | Component::Host(_)
                | Component::Any(_, _)
                | Component::Nesting => {
                    bail!("Component `{component:?}` is not supported: {root_selector:?}");
                }

                // When we find a combinator, we dump all stored components.
                Component::Combinator(_) => {
                    components.extend(combinator_early.drain(..));
                    components.extend(combinator_late.drain(..));
                    components.push(component.clone());
                }

                // We handle these components specially
                Component::NonTSPseudoClass(_) => {
                    // we filter out pseudo-classes as they aren't available in a static DOM
                    conditional = true;
                }
                Component::PseudoElement(elem) => {
                    // mark a pseudo-element properly
                    ensure!(pseudo_element.is_none(), "Duplicate pseudo element found.");
                    pseudo_element = Some(elem.clone());
                }

                // Push all components relating to the base element first.
                //
                // The `scrapers` crate does not support selectors like `:is(#a)div` even though
                // this is valid CSS.
                Component::ExplicitAnyNamespace
                | Component::ExplicitNoNamespace
                | Component::DefaultNamespace(_)
                | Component::Namespace(_, _)
                | Component::ExplicitUniversalType
                | Component::LocalName(_)
                | Component::Root => combinator_early.push(component.clone()),

                // Handle all other components
                Component::ID(_)
                | Component::Class(_)
                | Component::AttributeInNoNamespaceExists { .. }
                | Component::AttributeInNoNamespace { .. }
                | Component::AttributeOther(_)
                | Component::Empty
                | Component::Nth(_)
                | Component::NthOf(_)
                | Component::Has(_) => combinator_late.push(component.clone()),

                Component::Negation(selectors)
                | Component::Where(selectors)
                | Component::Is(selectors) => {
                    let mut new = Vec::new();
                    for selector in selectors {
                        let parsed = filter_selector(root_selector, selector)?;
                        if parsed.is_conditional {
                            conditional = true;
                        }
                        new.push(parsed.selector);
                    }
                    let boxed: Box<[lightningcss::selector::Selector]> = new.into();
                    match component {
                        Component::Negation(_) => combinator_late.push(Component::Negation(boxed)),
                        Component::Where(_) => combinator_late.push(Component::Where(boxed)),
                        Component::Is(_) => combinator_late.push(Component::Is(boxed)),
                        _ => unreachable!(),
                    }
                }
            }
        }
        components.extend(combinator_early.drain(..));
        components.extend(combinator_late.drain(..));

        Ok(FilteredSelector {
            selector: lightningcss::selector::Selector::from(components),
            is_conditional: conditional,
            pseudo_element: pseudo_element.map(|x| CssParserToString::to_css_string(&x)),
            specificity: root_selector.specificity(),
        })
    }

    /// Generates the list of rules for a single style rule declaration.
    fn generate_rules(
        out: &mut Vec<Arc<RawCssRule>>,
        style: &StyleRule,
        force_conditional: bool,
    ) -> Result<()> {
        if let Some(declarations) = parse_declarations(&style.declarations)? {
            let declarations = Arc::new(declarations);
            for selector in &style.selectors.0 {
                let filtered = filter_selector(selector, selector)?;
                let new_selector_str =
                    ToCss::to_css_string(&filtered.selector, PrinterOptions::default())?;

                if new_selector_str.is_empty() {
                    info!("Ignoring unknown selector: {selector:?}");
                    continue;
                }

                let raw = RawCssRule {
                    selector: Arc::new(Selectors::compile(&new_selector_str).map_err(|()| {
                        Error::msg(format!("Selector is not valid: {new_selector_str}"))
                    })?),
                    is_conditional: force_conditional | filtered.is_conditional,
                    pseudo_element: filtered.pseudo_element.map(Into::into),
                    declarations: declarations.clone(),
                    specificity: filtered.specificity,
                };
                out.push(Arc::new(raw));
            }
        }
        Ok(())
    }

    /// Applies the `force_conditional` flag to
    fn apply_force_conditional(
        out: &mut Vec<Arc<RawCssRule>>,
        orig_list: &[Arc<RawCssRule>],
        force_conditional: bool,
    ) {
        for rule in orig_list {
            if !rule.is_conditional && force_conditional {
                out.push(Arc::new(RawCssRule {
                    selector: rule.selector.clone(),
                    is_conditional: true,
                    pseudo_element: rule.pseudo_element.clone(),
                    declarations: rule.declarations.clone(),
                    specificity: rule.specificity,
                }));
            } else {
                out.push(rule.clone());
            }
        }
    }

    /// The main recursive function that handles parsing rules.
    ///
    /// This is recursive to allow for handling media queries and import statements.
    #[async_recursion]
    async fn push_rules(
        out: &mut Vec<Arc<RawCssRule>>,
        rules: &CssRuleList<'_>,
        root: &RelaWebroot,
        force_conditional: bool,
        cache: &CssCache,
    ) -> Result<()> {
        for rule in &rules.0 {
            match rule {
                CssRule::Media(media_query) => {
                    let is_conditional = force_conditional || !media_query.query.always_matches();
                    push_rules(out, &media_query.rules, root, is_conditional, cache).await?
                }
                // @import is *not* cached for ease of coding.
                //
                // Assumption: @import is not used heavily for large shared stylesheets. This
                // should be fairly rare with the kind of static sites extract-web is meant for.
                CssRule::Import(import_statement) => {
                    let url: &str = &import_statement.url;
                    match root.load_rela(url).await {
                        Ok((data, new_root)) => {
                            let parsed = cache.get_css(data, &new_root).await?;
                            apply_force_conditional(out, &parsed, force_conditional);
                        }
                        Err(e) => warn!("Could not load '{url}': {e}"),
                    }
                }
                CssRule::Style(style) => {
                    if !style.rules.0.is_empty() {
                        warn!("Nested CSS rules are not supported!!");
                    }
                    if let Err(e) = generate_rules(out, style, force_conditional) {
                        warn!("Rules ignored: {e}");
                    }
                }
                CssRule::FontFace(_) => warn!("Preexisting @font-face exists."),
                css => warn!("CSS rule not recognized: {css:?}"),
            }
        }
        Ok(())
    }

    let mut rules = Vec::new();
    let parsed = StyleSheet::parse(data, ParserOptions::default()).map_err(|x| x.into_owned())?;
    push_rules(&mut rules, &parsed.rules, root, false, cache).await?;
    Ok(rules)
}

#[derive(Debug, Clone)]
pub struct CssCache {
    cache: Arc<Cache<(ArcStr, Arc<Path>), Arc<[Arc<RawCssRule>]>, WyHashBuilder>>,
}
impl CssCache {
    pub fn new() -> Self {
        CssCache {
            cache: Arc::new(CacheBuilder::new(CACHE_SIZE).build_with_hasher(Default::default())),
        }
    }

    pub async fn get_css(
        &self,
        source: ArcStr,
        root: &RelaWebroot,
    ) -> Result<Arc<[Arc<RawCssRule>]>> {
        let root_name: Cow<str> = match root.file_name().file_name() {
            None => Cow::Borrowed("<unknown>"),
            Some(name) => name.to_string_lossy(),
        };
        let root_name: &str = &root_name;
        let span = info_span!("parse_css", name = root_name);

        match self
            .cache
            .try_get_with(
                (source.clone(), root.rela_key().clone()),
                async {
                    match parse_css(&source, &root, self).await {
                        Ok(val) => Ok(val.into()),
                        Err(e) => Err(e.to_string()),
                    }
                }
                .instrument(span),
            )
            .await
        {
            Ok(x) => Ok(x),
            Err(e) => bail!("Parsing failed: {e}"),
        }
    }
}
