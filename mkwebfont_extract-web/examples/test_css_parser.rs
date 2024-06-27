use anyhow::Result;
use mkwebfont_common::FILTER_SPEC;
use std::{io, path::PathBuf};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(FILTER_SPEC)
        .with_writer(io::stderr)
        .init();

    let extractor = mkwebfont_extract_web::WebrootInfoExtractor::new();
    extractor
        .push_webroot(&PathBuf::from(std::env::args().skip(1).next().unwrap()), &[])
        .await?;

    let info = extractor.build().await;
    println!("{:#?}", info);
    for stack in &info.font_stacks {
        println!("{:?} => {:?}", stack.stack, stack.glyphs());
    }

    Ok(())
}
