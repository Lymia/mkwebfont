use lightningcss::stylesheet::{ParserOptions, StyleSheet};
use mkwebfont_common::FILTER_SPEC;
use scraper::Html;
use std::{io, path::PathBuf};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(FILTER_SPEC)
        .with_writer(io::stderr)
        .init();

    let data = std::fs::read_to_string(std::env::args().skip(1).next().unwrap()).unwrap();
    //let test = StyleSheet::parse(&data, ParserOptions::default()).unwrap();
    let test = mkwebfont_extract_web::raw_rules(
        &Html::parse_document(&data),
        &mkwebfont_extract_web::Webroot::new(PathBuf::from(
            "/home/aino/Projects/writing/Website/build/web/",
        ))
        .unwrap()
        .rela(&PathBuf::from("index.html"))
        .await
        .unwrap(),
        &[],
    )
    .await
    .unwrap();
    println!("{test:#?}");
}
