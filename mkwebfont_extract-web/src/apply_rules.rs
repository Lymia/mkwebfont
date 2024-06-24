use crate::gather_css::{parse_declarations, RawCssRule, RawCssRuleDeclarations};
use anyhow::{Error, Result};
use arcstr::ArcStr;
use ego_tree::NodeId;
use enumset::{EnumSet, EnumSetType};
use lightningcss::{
    declaration::DeclarationBlock,
    properties::font::{AbsoluteFontWeight, FontStyle},
    stylesheet::ParserOptions,
};
use mkwebfont_common::hashing::WyHashBuilder;
use scraper::{selectable::Selectable, Element, ElementRef, Html, Selector};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, LazyLock},
};
use tracing::warn;

#[derive(Debug, Default)]
struct NodeProperties {
    font_stack: HashSet<Arc<[ArcStr]>, WyHashBuilder>,
    font_weight: HashSet<i32, WyHashBuilder>,
    font_style: HashSet<ParsedFontStyle, WyHashBuilder>,
    is_displayed: Option<bool>,
    content: HashSet<ArcStr, WyHashBuilder>,
    cleared: EnumSet<ClearedFlags>,
}

#[derive(Debug, Default)]
struct NodeInfo {
    properties: NodeProperties,
    pseudo_elements: HashMap<ArcStr, NodeProperties>,
}

#[derive(Ord, PartialOrd, Eq, PartialEq, Hash, Debug, Copy, Clone)]
pub enum ParsedFontStyle {
    Normal,
    Italic,
    Oblique,
}

#[derive(EnumSetType, Debug)]
enum ClearedFlags {
    FontStack,
    FontWeight,
    FontStyle,
    Content,
}

#[derive(Debug, Default)]
pub struct RawNodeInfo {
    raw: HashMap<NodeId, NodeInfo, WyHashBuilder>,
}

/// Applies a CSS rule to an element.
fn apply_rule_to_elem(elem: &mut NodeInfo, rule: &RawCssRule) {
    if let Some(pseudo) = &rule.pseudo_element {
        let target = elem.pseudo_elements.entry(pseudo.clone()).or_default();
        apply_properties(target, rule.is_conditional, &rule.declarations)
    } else {
        apply_properties(&mut elem.properties, rule.is_conditional, &rule.declarations)
    }
}

/// Applies properties to an element.
fn apply_properties(
    properties: &mut NodeProperties,
    is_conditional: bool,
    decls: &RawCssRuleDeclarations,
) {
    // Handle `font-family`.
    if let Some(stack) = &decls.font_stack {
        if !is_conditional {
            properties.font_stack.clear();
            properties.cleared.insert(ClearedFlags::FontStack);
        }
        properties.font_stack.insert(stack.clone());
    }

    // Handle `font-weight`.
    if let Some(weight) = &decls.font_weight {
        if !is_conditional {
            properties.font_weight.clear();
            properties.cleared.insert(ClearedFlags::FontWeight);
        }
        properties.font_weight.insert(match weight {
            AbsoluteFontWeight::Weight(w) => *w as i32,
            AbsoluteFontWeight::Normal => 400,
            AbsoluteFontWeight::Bold => 700,
        });
    }

    // Handle `font-style`.
    if let Some(style) = &decls.font_style {
        if !is_conditional {
            properties.font_style.clear();
            properties.cleared.insert(ClearedFlags::FontStyle);
        }
        properties.font_style.insert(match style {
            FontStyle::Normal => ParsedFontStyle::Normal,
            FontStyle::Italic => ParsedFontStyle::Italic,
            FontStyle::Oblique(_) => ParsedFontStyle::Oblique,
        });
    }

    // Handle `display`.
    if decls.is_displayed == Some(false) && !is_conditional {
        properties.is_displayed = Some(false);
    } else if decls.is_displayed == Some(true) {
        properties.is_displayed = Some(true);
    }

    // Handle `content`.
    if let Some(content) = &decls.content {
        if !is_conditional {
            properties.content.clear();
            properties.cleared.insert(ClearedFlags::Content);
        }
        properties.content.insert(content.clone());
    }
}

/// Applies a CSS rule to a document.
fn apply_rule_to_raw_info(info: &mut RawNodeInfo, document: &Html, rule: &RawCssRule) {
    for elem in document.select(&rule.selector) {
        apply_rule_to_elem(info.raw.entry(elem.id()).or_default(), rule);
    }
}

#[derive(Debug, Default, Clone)]
pub struct ResolvedNodeProperties {
    pub font_stack: HashSet<Arc<[ArcStr]>, WyHashBuilder>,
    pub font_weight: HashSet<i32, WyHashBuilder>,
    pub font_style: HashSet<ParsedFontStyle, WyHashBuilder>,
    pub content: HashSet<ArcStr, WyHashBuilder>,
}
impl ResolvedNodeProperties {
    fn apply_props(&mut self, props: &NodeProperties) {
        if props.cleared.contains(ClearedFlags::FontStack) {
            self.font_stack.clear();
        }
        self.font_stack.extend(props.font_stack.iter().cloned());

        if props.cleared.contains(ClearedFlags::FontWeight) {
            self.font_weight.clear();
        }
        self.font_weight.extend(props.font_weight.iter().cloned());

        if props.cleared.contains(ClearedFlags::FontStyle) {
            self.font_style.clear();
        }
        self.font_style.extend(props.font_style.iter().cloned());

        // note: content isn't inherited
    }
}

#[derive(Debug, Default)]
pub struct ResolvedNode {
    pub properties: ResolvedNodeProperties,
    pub pseudo_elements: HashMap<ArcStr, ResolvedNodeProperties>,
}

impl RawNodeInfo {
    pub fn compute(document: &Html, rules: &[Arc<RawCssRule>]) -> Result<Self> {
        static SELECTOR: LazyLock<Selector> =
            LazyLock::new(|| Selector::parse("*[style]").unwrap());
        let mut info = Self::default();
        for rule in rules {
            apply_rule_to_raw_info(&mut info, document, &rule);
        }
        for elem in document.select(&SELECTOR) {
            let style = elem.attr("style").unwrap();
            match DeclarationBlock::parse_string(style, ParserOptions::default()) {
                Ok(block) => {
                    if let Some(decls) = parse_declarations(&block)? {
                        apply_properties(
                            &mut info.raw.entry(elem.id()).or_default().properties,
                            false,
                            &decls,
                        );
                    }
                }
                Err(e) => warn!("Error parsing style {style:?}: {e}"),
            }
        }
        Ok(info)
    }

    pub fn is_displayed(&self, node: &ElementRef) -> bool {
        let mut accum = Some(node.clone());
        while let Some(x) = accum {
            if let Some(props) = self.raw.get(&x.id()) {
                if props.properties.is_displayed == Some(false) {
                    return false;
                }
            }
            accum = x.parent_element();
        }
        true
    }

    pub fn has_pseudo_elements(&self, node: &ElementRef) -> bool {
        if let Some(props) = self.raw.get(&node.id()) {
            !props.pseudo_elements.is_empty()
        } else {
            false
        }
    }

    pub fn resolve_node(&self, node: &ElementRef) -> ResolvedNode {
        let mut accum = Some(node.clone());
        let mut chain = Vec::new();
        while let Some(x) = accum {
            chain.push(x.clone());
            accum = x.parent_element();
        }

        let mut resolved = ResolvedNodeProperties::default();
        for node in chain.into_iter().rev() {
            if let Some(props) = self.raw.get(&node.id()) {
                resolved.apply_props(&props.properties);
            }
        }
        let mut pseudo_elements = HashMap::default();
        if let Some(props) = self.raw.get(&node.id()) {
            for (k, v) in &props.pseudo_elements {
                let mut pelem_resolved = resolved.clone();
                pelem_resolved.apply_props(v);
                pelem_resolved.content.extend(v.content.iter().cloned());
                pseudo_elements.insert(k.clone(), pelem_resolved);
            }
            resolved
                .content
                .extend(props.properties.content.iter().cloned());
        }

        ResolvedNode { properties: resolved, pseudo_elements }
    }
}
