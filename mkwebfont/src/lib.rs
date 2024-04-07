mod font_ops;
mod gf_ranges;
mod nix_base32;
mod ranges;
mod splitter;
mod woff2;

pub use font_ops::{FontStyle, FontWeight};
pub use splitter::{FontStylesheetEntry, FontStylesheetInfo};

pub fn split_webfont(
    font_path: &std::path::Path,
    store_path: &std::path::Path,
) -> anyhow::Result<FontStylesheetInfo> {
    splitter::split_webfont(
        None,
        ranges::WebfontDataCtx::load(),
        &std::fs::read(font_path)?,
        store_path,
    )
}
