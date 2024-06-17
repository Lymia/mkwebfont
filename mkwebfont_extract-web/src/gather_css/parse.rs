use crate::RelaWebroot;
use anyhow::Result;
use async_recursion::async_recursion;
use lightningcss::{
    rules::{CssRule, CssRuleList},
    stylesheet::{ParserOptions, StyleSheet},
};
use scraper::Selector;
use std::collections::HashMap;
use tracing::warn;

#[derive(Clone, Debug)]
struct ParsedCss(Vec<ParsedCssRule>);

#[derive(Clone, Debug)]
struct FontStack(Vec<String>);

#[derive(Clone, Debug)]
struct ParsedCssRule {
    selector: Selector,
    used_fonts: Vec<FontStack>,
    is_displayed: bool,
    pseudo_element_content: Vec<ParsedPseudoElement>,
    specificity: u32,
}

#[derive(Clone, Debug)]
struct ParsedPseudoElement {
    used_fonts: Vec<FontStack>,
    content: String,
}

struct CssCache {
    arc_cache: HashMap<usize, ParsedCssRule>,
}

async fn parse_css(data: &str, root: &RelaWebroot) -> Result<ParsedCss> {
    struct RawCssRule {
        selector: Selector,
        stack: FontStack,
        is_conditional: bool,
        is_displayed: bool,
        is_pseudo_element: bool,
        pseudo_element_content: Option<String>,
        specificity: u32,
    }

    #[async_recursion]
    async fn push_rules(
        out: &mut Vec<RawCssRule>,
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
                CssRule::Style(_) => {}
                CssRule::FontFace(_) => warn!("Preexisting @font-face exists."),
                css => warn!("CSS rule not recognized: {css:?}"),
            }
        }
        Ok(())
    }

    let mut rules = Vec::new();
    let parsed = StyleSheet::parse(data, ParserOptions::default()).map_err(|x| x.into_owned())?;
    push_rules(&mut rules, &parsed.rules, root, false).await?;

    todo!()
}
