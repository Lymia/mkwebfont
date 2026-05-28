use lightningcss::properties::font::FamilyName;
use lightningcss::traits::IntoOwned;
use serde_json::Value;
use std::io::Cursor;
use tsugiki::dom::{Node, NodeRef};

pub fn inner_html(node: &NodeRef) -> String {
    let mut str = Vec::new();
    for child in node.children() {
        child.serialize(&mut Cursor::new(&mut str)).unwrap();
    }
    String::from_utf8(str).unwrap()
}

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Debug, Hash)]
pub struct NodeId(usize);
impl NodeId {
    pub fn from_node(node: &NodeRef) -> NodeId {
        NodeId((&*node.0) as *const Node as usize)
    }
}

pub fn condense_spaces(s: &str) -> String {
    let mut out = String::new();

    let mut last_whitespace = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !last_whitespace {
                out.push(' ');
            }
            last_whitespace = true;
        } else {
            out.push(ch);
            last_whitespace = false;
        }
    }

    out.trim().to_string()
}

pub fn extract_family_name(name: &FamilyName<'_>) -> String {
    let value = serde_json::to_value(name).unwrap();
    match value {
        Value::String(s) => s,
        _ => unreachable!(),
    }
}

pub fn create_family_name(name: &str) -> FamilyName<'static> {
    // TODO: This is very slow, but hopefully lightningcss people fix this shit.
    let json = serde_json::to_string(name).unwrap();
    let value: FamilyName = serde_json::from_str(&json).unwrap();
    value.into_owned()
}
