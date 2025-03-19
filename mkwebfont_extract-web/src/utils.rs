use kuchikiki::{Node, NodeRef};
use std::io::Cursor;

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
