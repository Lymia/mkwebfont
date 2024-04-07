mod font_ops;
mod gf_ranges;
mod nix_base32;
mod ranges;
mod splitter;
mod woff2;

pub use font_ops::{FontStyle, FontWeight};
pub use splitter::{FontStylesheetEntry, FontStylesheetInfo};

pub fn split_webfont(path: std::path::PathBuf) -> anyhow::Result<FontStylesheetInfo> {
    splitter::split_webfont(
        None,
        ranges::WebfontDataCtx::load(),
        &std::fs::read(path)?,
        &std::path::PathBuf::from("run"),
    )
}
