use crate::apply_rules::{ParsedFontStyle, ResolvedNodeProperties};
use arcstr::ArcStr;
use enumset::{EnumSet, EnumSetType};
use mkwebfont_common::hashing::WyHashBuilder;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Debug, Clone)]
pub struct TextInfo {
    pub data: Vec<FontStackInfo>,
}

#[derive(Debug, Clone)]
pub struct FontStackInfo {
    pub stack: Arc<[ArcStr]>,
    pub samples: Vec<TextSample>,
}
impl FontStackInfo {
    pub fn glyphs(&self) -> String {
        let mut chars = HashSet::new();
        for sample in &self.samples {
            chars.extend(sample.glyphs().chars());
        }

        let mut chars: Vec<_> = chars.into_iter().collect();
        chars.sort();
        chars.into_iter().collect()
    }
}

#[derive(Debug, Clone)]
pub struct TextSample {
    pub used_styles: EnumSet<TextSampleStyle>,
    pub used_weights: Arc<[i32]>,
    pub content: Vec<ArcStr>,
}
impl TextSample {
    pub fn glyphs(&self) -> String {
        let chars: HashSet<_> = self
            .content
            .iter()
            .flat_map(|x| x.as_str().chars())
            .collect();
        let mut chars: Vec<_> = chars.into_iter().collect();
        chars.sort();
        chars.into_iter().collect()
    }
}

#[derive(EnumSetType, Debug)]
pub enum TextSampleStyle {
    Normal,
    Italics,
    Oblique,
}

#[derive(Debug, Default)]
pub struct TextInfoBuilder {
    stacks: HashMap<
        Arc<[ArcStr]>,
        HashMap<TextSampleKey, HashSet<ArcStr, WyHashBuilder>>,
        WyHashBuilder,
    >,
    cached_strs: HashSet<ArcStr, WyHashBuilder>,
    cached_stacks: HashSet<Arc<[ArcStr]>, WyHashBuilder>,
    cached_weights: HashSet<Arc<[i32]>, WyHashBuilder>,
}
impl TextInfoBuilder {
    fn intern_str(&mut self, str: &str) -> ArcStr {
        if let Some(x) = self.cached_strs.get(str) {
            x.clone()
        } else {
            let arc = ArcStr::from(str);
            self.cached_strs.insert(arc.clone());
            arc
        }
    }

    fn intern_stack(&mut self, str: &[String]) -> Arc<[ArcStr]> {
        let arc: Arc<[_]> = str.iter().map(ArcStr::from).collect();
        if let Some(x) = self.cached_stacks.get(&arc) {
            x.clone()
        } else {
            self.cached_stacks.insert(arc.clone());
            arc
        }
    }

    fn intern_weights(&mut self, weights: &HashSet<i32, WyHashBuilder>) -> Arc<[i32]> {
        let mut weights: Vec<_> = weights.iter().cloned().collect();
        weights.sort();
        let arc: Arc<[_]> = weights.into();
        if let Some(x) = self.cached_weights.get(&arc) {
            x.clone()
        } else {
            self.cached_weights.insert(arc.clone());
            arc
        }
    }

    pub fn push_sample(&mut self, properties: &ResolvedNodeProperties, additional_text: &[ArcStr]) {
        let key = TextSampleKey {
            styles: properties
                .font_style
                .iter()
                .map(|x| match x {
                    ParsedFontStyle::Normal => TextSampleStyle::Normal,
                    ParsedFontStyle::Italic => TextSampleStyle::Italics,
                    ParsedFontStyle::Oblique => TextSampleStyle::Oblique,
                })
                .collect(),
            weights: self.intern_weights(&properties.font_weight),
        };
        let content: Vec<_> = additional_text
            .iter()
            .chain(additional_text.iter())
            .filter(|x| !x.is_empty())
            .map(|x| self.intern_str(&x))
            .collect();

        for stack in &properties.font_stack {
            let stack = self.intern_stack(&stack);
            let texts = self
                .stacks
                .entry(stack)
                .or_default()
                .entry(key.clone())
                .or_default();
            texts.extend(content.iter().cloned());
        }
    }

    pub fn build(&self) -> TextInfo {
        let mut keys: Vec<_> = self.stacks.keys().collect();
        keys.sort();

        let mut out = TextInfo { data: vec![] };
        for key in keys {
            let stack = self.stacks.get(key).unwrap();
            let mut stack_keys: Vec<_> = stack.keys().collect();
            stack_keys.sort();

            let mut stack_info = FontStackInfo { stack: key.clone(), samples: vec![] };
            for key in stack_keys {
                let mut content: Vec<_> = stack.get(key).unwrap().into_iter().cloned().collect();
                content.sort();
                stack_info.samples.push(TextSample {
                    used_styles: key.styles,
                    used_weights: key.weights.clone(),
                    content,
                });
            }
            out.data.push(stack_info);
        }
        out
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
struct TextSampleKey {
    styles: EnumSet<TextSampleStyle>,
    weights: Arc<[i32]>,
}
