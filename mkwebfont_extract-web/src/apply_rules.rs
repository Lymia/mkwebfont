use crate::{
    gather_css::{parse_declarations, ParsedCssRule, RawCssRule, RawCssRuleDeclarations},
    utils::NodeId,
};
use anyhow::Result;
use arcstr::ArcStr;
use kuchikiki::{traits::NodeIterator, NodeRef, Selectors};
use lightningcss::{
    declaration::DeclarationBlock,
    properties::font::{AbsoluteFontWeight, FontStyle},
    stylesheet::ParserOptions,
};
use mkwebfont_common::hashing::{WyHashBuilder, WyHashSet};
use std::{
    collections::HashMap,
    hash::Hash,
    sync::{Arc, LazyLock},
};
use tracing::warn;

#[derive(Debug)]
struct NodeProperty<T> {
    active: WyHashSet<T>,
    overwritten: bool,
}
impl<T> NodeProperty<T> {
    fn push_node(&mut self, rule: &ParsedCssRule<T>, is_conditional: bool)
    where T: Clone + Hash + Eq {
        match rule {
            ParsedCssRule::Override(new) => {
                self.active.insert(new.clone());
                if !is_conditional {
                    self.overwritten = true;
                }
            }
            ParsedCssRule::OverrideUnset => {
                if !is_conditional {
                    self.active.clear();
                }
                self.overwritten = true;
            }
            ParsedCssRule::Inherit => {
                if !is_conditional {
                    self.active.clear();
                    self.overwritten = false;
                }
            }
            ParsedCssRule::IgnoreSet => {
                if !is_conditional {
                    self.active.clear();
                    self.overwritten = true;
                }
            }
            ParsedCssRule::NoneSet => {}
        }
    }
}
impl<T> Default for NodeProperty<T> {
    fn default() -> Self {
        NodeProperty { active: Default::default(), overwritten: false }
    }
}

#[derive(Debug, Default)]
struct NodeProperties {
    font_stack: NodeProperty<Arc<[ArcStr]>>,
    font_weight: NodeProperty<i32>,
    font_style: NodeProperty<ParsedFontStyle>,
    is_displayed: NodeProperty<bool>,
    content: NodeProperty<ArcStr>,
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
    properties
        .font_stack
        .push_node(&decls.font_stack, is_conditional);
    properties.font_weight.push_node(
        &decls.font_weight.map(|x| match x {
            AbsoluteFontWeight::Weight(w) => *w as i32,
            AbsoluteFontWeight::Normal => 400,
            AbsoluteFontWeight::Bold => 700,
        }),
        is_conditional,
    );
    properties.font_style.push_node(
        &decls.font_style.map(|x| match x {
            FontStyle::Normal => ParsedFontStyle::Normal,
            FontStyle::Italic => ParsedFontStyle::Italic,
            FontStyle::Oblique(_) => ParsedFontStyle::Oblique,
        }),
        is_conditional,
    );
    properties
        .is_displayed
        .push_node(&decls.is_displayed, is_conditional);
    properties.content.push_node(&decls.content, is_conditional);
}

/// Applies a CSS rule to a document.
fn apply_rule_to_raw_info(info: &mut RawNodeInfo, document: &NodeRef, rule: &RawCssRule) {
    for elem in rule
        .selector
        .filter(document.inclusive_descendants().elements())
    {
        apply_rule_to_elem(
            info.raw
                .entry(NodeId::from_node(elem.as_node()))
                .or_default(),
            rule,
        );
    }
}

#[derive(Debug, Default, Clone)]
pub struct ResolvedNodeProperties {
    pub font_stack: WyHashSet<Arc<[ArcStr]>>,
    pub font_weight: WyHashSet<i32>,
    pub font_style: WyHashSet<ParsedFontStyle>,
    pub content: WyHashSet<ArcStr>,
}
impl ResolvedNodeProperties {
    fn apply_props(&mut self, props: &NodeProperties) {
        fn push_property<T: Hash + Eq + Clone>(set: &mut WyHashSet<T>, props: &NodeProperty<T>) {
            if props.overwritten {
                set.clear();
            }
            set.extend(props.active.iter().cloned());
        }

        push_property(&mut self.font_stack, &props.font_stack);
        push_property(&mut self.font_weight, &props.font_weight);
        push_property(&mut self.font_style, &props.font_style);
        // note: content isn't inherited
    }
}

#[derive(Debug, Default)]
pub struct ResolvedNode {
    pub properties: ResolvedNodeProperties,
    pub pseudo_elements: HashMap<ArcStr, ResolvedNodeProperties>,
}

impl RawNodeInfo {
    pub fn compute(document: &NodeRef, rules: &[Arc<RawCssRule>]) -> Result<Self> {
        static SELECTOR: LazyLock<Selectors> =
            LazyLock::new(|| Selectors::compile("*[style]").unwrap());

        let mut info = Self::default();
        for rule in rules {
            apply_rule_to_raw_info(&mut info, document, &rule);
        }
        for elem in SELECTOR.filter(document.inclusive_descendants().elements()) {
            let style = elem.attributes.borrow();
            let style = style.get("style").unwrap();
            match DeclarationBlock::parse_string(style, ParserOptions::default()) {
                Ok(block) => {
                    if let Some(decls) = parse_declarations(&block)? {
                        apply_properties(
                            &mut info
                                .raw
                                .entry(NodeId::from_node(elem.as_node()))
                                .or_default()
                                .properties,
                            false,
                            &decls,
                        );
                    }
                }
                Err(e) => warn!("Error parsing style {style:?}: {e}"),
            };
        }
        Ok(info)
    }

    pub fn resolve_node(&self, node: &NodeRef) -> ResolvedNode {
        let mut accum = Some(node.clone());
        let mut chain = Vec::new();
        while let Some(x) = accum {
            chain.push(x.clone());
            accum = x.parent();
        }

        let mut resolved = ResolvedNodeProperties::default();
        for node in chain.into_iter().rev() {
            if let Some(props) = self.raw.get(&NodeId::from_node(&node)) {
                resolved.apply_props(&props.properties);
            }
        }
        let mut pseudo_elements = HashMap::default();
        if let Some(props) = self.raw.get(&NodeId::from_node(&node)) {
            for (k, v) in &props.pseudo_elements {
                let mut pelem_resolved = resolved.clone();
                pelem_resolved.apply_props(v);
                pelem_resolved
                    .content
                    .extend(v.content.active.iter().cloned());
                pseudo_elements.insert(k.clone(), pelem_resolved);
            }
            resolved
                .content
                .extend(props.properties.content.active.iter().cloned());
        }

        ResolvedNode { properties: resolved, pseudo_elements }
    }
}
