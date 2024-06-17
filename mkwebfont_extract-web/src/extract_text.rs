use crate::{
    apply_rules::RawNodeInfo, font_info::TextInfoBuilder, gather_css::CssCache,
    webroot::RelaWebroot,
};
use anyhow::Result;
use arcstr::ArcStr;
use scraper::Html;
use std::sync::Arc;
use tokio::sync::RwLock;

// TODO: Try to not fragment text samples so much as we do here. Will help later with ligatures.

pub async fn extract_text(
    data: &ArcStr,
    root: &RelaWebroot,
    css_cache: &CssCache,
    inject_css: &[ArcStr],
    builder: Arc<RwLock<TextInfoBuilder>>,
) -> Result<()> {
    let document = Html::parse_document(&data);
    let rules = css_cache
        .get_rules_from_document(&document, root, inject_css)
        .await?;
    let node_info = RawNodeInfo::compute(&document, &rules);

    for element in document.root_element().descendent_elements() {
        let has_text = element.children().any(|x| x.value().is_text());

        if (has_text || node_info.has_pseudo_elements(&element)) && node_info.is_displayed(&element)
        {
            let resolved = node_info.resolve_node(&element);

            let mut retrieved_text: Vec<_> = element
                .children()
                .flat_map(|x| x.value().as_text())
                .map(|x| ArcStr::from(String::from(&x.text)))
                .collect();
            retrieved_text.extend(resolved.properties.content.iter().cloned());

            let pseudo_elements: Vec<(_, Vec<_>)> = resolved
                .pseudo_elements
                .iter()
                .map(|(_, v)| (v, v.content.iter().cloned().collect()))
                .collect();

            let mut lock = builder.write().await;
            lock.push_sample(&resolved.properties, &retrieved_text);
            for (properties, contents) in pseudo_elements {
                lock.push_sample(properties, &contents);
            }
        }
    }
    Ok(())
}
