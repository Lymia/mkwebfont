mod gf_ranges;
mod splitter;
mod subset;
mod woff2;

pub fn test(path: std::path::PathBuf) {
    splitter::test(path).unwrap()
}
