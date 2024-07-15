fn main() {
    build_harfbuzz();
}

fn build_harfbuzz() {
    cc::Build::new()
        .cpp(true)
        .flag("-std=c++11")
        .warnings(false)
        .file("harfbuzz/src/harfbuzz-subset.cc")
        .compile("embedded-harfbuzz-subset");

    println!("cargo:rerun-if-changed=harfbuzz/src");
}
