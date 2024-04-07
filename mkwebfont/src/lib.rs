use crate::ranges::WebfontDataCtx;

mod font_ops;
mod gf_ranges;
mod ranges;
mod splitter;
mod woff2;

pub fn split_webfont(path: std::path::PathBuf) -> anyhow::Result<()> {
    splitter::split_webfont(WebfontDataCtx::load(), path)
}
