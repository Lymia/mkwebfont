use scraper::ElementRef;

pub fn direct_text_children(node: &ElementRef) -> String {
    let mut text = String::new();
    for child in node.children() {
        let child = child.value();
        if child.is_text() {
            text.push_str(&String::from_utf8_lossy(child.as_text().unwrap().as_bytes()));
        }
    }
    text
}
