use crate::{
    apply_rules::RawNodeInfo, gather_css::CssCache, webroot::RelaWebroot,
    webroot_info::TextInfoBuilder,
};
use anyhow::Result;
use arcstr::ArcStr;
use mkwebfont_common::hashing::WyHashSet;
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
) -> Result<WyHashSet<Arc<[ArcStr]>>> {
    let rules = css_cache
        .get_rules_from_document(&data, root, inject_css)
        .await?;

    let mut samples = Vec::new();
    {
        let document = Html::parse_document(&data);
        let node_info = RawNodeInfo::compute(&document, &rules)?;
        for element in document.root_element().descendent_elements() {
            let has_text = element.children().any(|x| x.value().is_text());

            if (has_text || node_info.has_pseudo_elements(&element))
                && node_info.is_displayed(&element)
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
                    .into_iter()
                    .map(|(_, v)| {
                        let content = v.content.iter().cloned().collect();
                        (v, content)
                    })
                    .collect();

                samples.push((resolved.properties, retrieved_text));
                for (properties, contents) in pseudo_elements {
                    samples.push((properties, contents));
                }
            }
        }
    }

    let mut lock = builder.write().await;
    let mut stacks = WyHashSet::default();
    for (props, sample) in samples {
        stacks.extend(lock.push_sample(&props, &sample));
    }
    Ok(stacks)
}
