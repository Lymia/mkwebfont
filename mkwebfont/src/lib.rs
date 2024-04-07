mod allsorts_subset;
mod gf_ranges;
mod splitter;

pub fn test(path: std::path::PathBuf) {
    splitter::test(path).unwrap()
}
