use crate::webroot::RelaWebroot;
use anyhow::{bail, ensure, Error, Result};
use async_recursion::async_recursion;
use cssparser::ToCss as CssParserToString;
use lightningcss::{
    printer::PrinterOptions,
    properties::{
        custom::{CustomProperty, CustomPropertyName, Token, TokenOrValue},
        display::{Display, DisplayKeyword},
        font::FontFamily,
        Property,
    },
    rules::{style::StyleRule, CssRule, CssRuleList},
    selector::Component,
    stylesheet::{ParserOptions, StyleSheet},
    traits::ToCss,
};
use scraper::Selector;
use std::{hash::Hash, sync::Arc};
use tracing::warn;

// TODO: Figure out caching.

#[derive(Clone, Debug)]
pub struct RawCssRule {
    pub selector: Selector,
    pub is_conditional: bool,
    pub pseudo_element: Option<String>,
    pub declarations: Arc<RawCssRuleDeclarations>,
    pub specificity: u32,
}

#[derive(Clone, Debug)]
pub struct RawCssRuleDeclarations {
    pub stack: Option<Vec<String>>,
    pub is_displayed: Option<bool>,
    pub content: Option<String>,
}

/// Parses CSS data into a list of CSS rules.
pub async fn parse_css(data: &str, root: &RelaWebroot) -> Result<Vec<Arc<RawCssRule>>> {
    /// The result of filtering a selector.
    #[derive(Debug)]
    struct FilteredSelector<'a> {
        selector: lightningcss::selector::Selector<'a>,
        is_conditional: bool,
        pseudo_element: Option<String>,
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
                Component::Has(_) => {
                    warn!("`:has` is only partly supported: {root_selector:?}");
                    conditional = true;
                }
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
                Component::NonTSPseudoClass(cond) => {
                    // we filter out pseudo-classes as they aren't available in a static DOM
                    conditional = true;
                }
                Component::PseudoElement(elem) => {
                    // mark a pseudo-element properly
                    ensure!(pseudo_element.is_none());
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
                | Component::NthOf(_) => combinator_late.push(component.clone()),

                Component::Negation(selectors)
                | Component::Where(selectors)
                | Component::Is(selectors) => {
                    let mut new = Vec::new();
                    for selector in selectors {
                        let parsed = filter_selector(root_selector, selector)?;
                        conditional = true;
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
        })
    }

    /// Parses the list of declarations in a CSS rule into only the ones we need.
    fn parse_declarations(style: &StyleRule) -> Result<Option<RawCssRuleDeclarations>> {
        let mut raw_declarations =
            RawCssRuleDeclarations { stack: None, is_displayed: None, content: None };
        let mut is_interesting = false;

        if !style.declarations.important_declarations.is_empty() {
            warn!("`!important` is not handled correctly.");
        }

        for declaration in style
            .declarations
            .important_declarations
            .iter()
            .chain(style.declarations.declarations.iter())
        {
            /// Parses CSS font families
            fn parse_font_families(families: &[FontFamily<'_>]) -> Vec<String> {
                let mut new = Vec::new();
                for family in families {
                    match family {
                        FontFamily::Generic(_) => {
                            warn!("Generic font families are ignored: {family:?}")
                        }
                        FontFamily::FamilyName(name) => new.push(name.to_string()),
                    }
                }
                new
            }

            match declaration {
                Property::Display(kind) => {
                    if let Display::Keyword(DisplayKeyword::None) = kind {
                        raw_declarations.is_displayed = Some(false);
                    } else {
                        raw_declarations.is_displayed = Some(true);
                    }
                    is_interesting = true;
                }

                Property::Custom(CustomProperty {
                    name: CustomPropertyName::Unknown(name),
                    value,
                }) if *name == "content" => {
                    if value.0.len() == 1 {
                        match &value.0[0] {
                            TokenOrValue::Token(Token::String(str)) => {
                                raw_declarations.content = Some(str.to_string());
                                is_interesting = true;
                            }
                            _ => warn!("Could not parse `content` attribute: {value:?}"),
                        }
                    } else {
                        warn!("Could not parse `content` attribute: {value:?}");
                    }
                }

                Property::Font(font) => {
                    raw_declarations.stack = Some(parse_font_families(&font.family));
                    is_interesting = true;
                }
                Property::FontFamily(family) => {
                    raw_declarations.stack = Some(parse_font_families(&family));
                    is_interesting = true;
                }
                // TODO: Support for font styles.
                _ => {}
            }
        }

        if is_interesting {
            Ok(Some(raw_declarations))
        } else {
            Ok(None)
        }
    }

    /// Generates the list of rules for a single style rule declaration.
    fn generate_rules(
        out: &mut Vec<Arc<RawCssRule>>,
        style: &StyleRule,
        force_conditional: bool,
    ) -> Result<()> {
        if let Some(declarations) = parse_declarations(style)? {
            let declarations = Arc::new(declarations);
            for selector in &style.selectors.0 {
                let filtered = filter_selector(selector, selector)?;
                let new_selector_str =
                    ToCss::to_css_string(&filtered.selector, PrinterOptions::default())?;

                let mut raw = RawCssRule {
                    selector: Selector::parse(&new_selector_str)
                        .map_err(|e| Error::msg(format!("Failed to parse selector: {e}")))?,
                    is_conditional: force_conditional | filtered.is_conditional,
                    pseudo_element: filtered.pseudo_element,
                    declarations: declarations.clone(),
                    specificity: filtered.selector.specificity(),
                };
                out.push(Arc::new(raw));
            }
        }
        Ok(())
    }

    #[async_recursion]
    async fn push_rules(
        out: &mut Vec<Arc<RawCssRule>>,
        rules: &CssRuleList<'_>,
        root: &RelaWebroot,
        force_conditional: bool,
    ) -> Result<()> {
        for rule in &rules.0 {
            match rule {
                CssRule::Media(media_query) => {
                    let is_conditional = force_conditional || !media_query.query.always_matches();
                    push_rules(out, &media_query.rules, root, is_conditional).await?
                }
                CssRule::Import(import_statement) => {
                    let url: &str = &import_statement.url;
                    match root.load(url).await {
                        Ok(data) => {
                            let parsed = StyleSheet::parse(&data, ParserOptions::default())
                                .map_err(|x| x.into_owned())?;
                            let is_conditional =
                                force_conditional || !import_statement.media.always_matches();
                            push_rules(out, &parsed.rules, root, is_conditional).await?;
                        }
                        Err(e) => warn!("Could not load '{url}': {e}"),
                    }
                }
                CssRule::Style(style) => {
                    if !style.rules.0.is_empty() {
                        warn!("Nested CSS rules are not supported!!");
                    }
                    generate_rules(out, style, force_conditional)?;
                }
                CssRule::FontFace(_) => warn!("Preexisting @font-face exists."),
                css => warn!("CSS rule not recognized: {css:?}"),
            }
        }
        Ok(())
    }

    let mut rules = Vec::new();
    let parsed = StyleSheet::parse(data, ParserOptions::default()).map_err(|x| x.into_owned())?;
    push_rules(&mut rules, &parsed.rules, root, false).await?;
    Ok(rules)
}

#[derive(Hash, Eq, PartialEq, Debug)]
enum CacheKey {
    StaticStr(&'static str),
    ArcInstance(Arc<str>),
}
