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
