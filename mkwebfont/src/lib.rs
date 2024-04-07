mod subset;
mod gf_ranges;
mod splitter;
mod woff2;

pub fn test(path: std::path::PathBuf) {
    splitter::test(path).unwrap()
}
