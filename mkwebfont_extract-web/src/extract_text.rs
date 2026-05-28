use crate::apply_rules::{RawNodeInfo, ResolvedNodeProperties};
use crate::gather_css::CssCache;
use crate::utils;
use crate::webroot::RelaWebroot;
use crate::webroot_info::TextInfoBuilder;
use anyhow::Result;
use arcstr::ArcStr;
use mkwebfont_common::hashing::WyHashSet;
use std::mem::replace;
use std::sync::Arc;
use tokio::sync::RwLock;
use tsugiki::dom::{NodeData, NodeRef};

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
        let document = tsugiki::parse_document(data.as_str());
        let node_info = RawNodeInfo::compute(&document, &rules)?;

        fn push_samples(
            samples: &mut Vec<(ResolvedNodeProperties, Vec<ArcStr>)>,
            last_text_properties: &mut ResolvedNodeProperties,
            current_samples: &mut String,
        ) {
            if !current_samples.is_empty() {
                samples.push((last_text_properties.clone(), vec![
                    utils::condense_spaces(&replace(current_samples, String::new())).into(),
                ]));
            }
        }
        fn push_text(
            samples: &mut Vec<(ResolvedNodeProperties, Vec<ArcStr>)>,
            current_properties: &ResolvedNodeProperties,
            last_text_properties: &mut ResolvedNodeProperties,
            current_samples: &mut String,
            text: &str,
        ) {
            if current_properties.font_stack == last_text_properties.font_stack
                && current_properties.font_style == last_text_properties.font_style
                && current_properties.font_weight == last_text_properties.font_weight
            {
                current_samples.push_str(&text);
            } else {
                push_samples(samples, last_text_properties, current_samples);
                *last_text_properties = current_properties.clone();
                current_samples.push_str(&text);
            }
        }
        fn recurse(
            samples: &mut Vec<(ResolvedNodeProperties, Vec<ArcStr>)>,
            node: &NodeRef,
            node_info: &RawNodeInfo,
            current_properties: &mut ResolvedNodeProperties,
            last_text_properties: &mut ResolvedNodeProperties,
            current_samples: &mut String,
        ) {
            match node.0.data() {
                NodeData::Document(_) | NodeData::DocumentFragment => {
                    for child in node.children() {
                        recurse(
                            samples,
                            &child,
                            node_info,
                            current_properties,
                            last_text_properties,
                            current_samples,
                        );
                    }
                }
                NodeData::Element(_) => {
                    let resolved = node_info.resolve_node(node);

                    if resolved.properties.is_hidden {
                        return;
                    }

                    for (kind, props) in &resolved.pseudo_elements {
                        if (kind != ":before" && kind != ":after") || props.content.len() != 1 {
                            let content: Vec<_> = props.content.iter().cloned().collect();
                            samples.push((props.clone(), content));
                        }
                    }

                    if let Some(props) = resolved.pseudo_elements.get(":before") {
                        if props.content.len() == 1 {
                            push_text(
                                samples,
                                props,
                                last_text_properties,
                                current_samples,
                                props.content.iter().next().unwrap().as_str(),
                            )
                        }
                    }

                    // Handle replacing the properties.
                    let previous_properties =
                        replace(current_properties, resolved.properties.clone());
                    for child in node.children() {
                        recurse(
                            samples,
                            &child,
                            node_info,
                            current_properties,
                            last_text_properties,
                            current_samples,
                        );
                    }
                    *current_properties = previous_properties;

                    if let Some(props) = resolved.pseudo_elements.get(":after") {
                        if props.content.len() == 1 {
                            push_text(
                                samples,
                                props,
                                last_text_properties,
                                current_samples,
                                props.content.iter().next().unwrap().as_str(),
                            )
                        }
                    }
                }
                NodeData::Text(text) => {
                    push_text(
                        samples,
                        current_properties,
                        last_text_properties,
                        current_samples,
                        text.borrow().content.as_str(),
                    );
                }
                _ => {}
            }
        }

        let mut current_properties = ResolvedNodeProperties::default();
        let mut last_text_properties = ResolvedNodeProperties::default();
        let mut current_samples = String::new();
        recurse(
            &mut samples,
            &document,
            &node_info,
            &mut current_properties,
            &mut last_text_properties,
            &mut current_samples,
        );
        push_samples(&mut samples, &mut last_text_properties, &mut current_samples);
    }

    let mut lock = builder.write().await;
    let mut stacks = WyHashSet::default();
    for (props, sample) in samples {
        stacks.extend(lock.push_text_sample(root.file_name().clone(), &props, &sample));
    }
    Ok(stacks)
}
