mod contrib;
mod fonts;
mod ranges;
mod splitter;

pub use fonts::{FontStyle, FontWeight};
pub use splitter::{FontStylesheetEntry, FontStylesheetInfo};

pub fn split_webfont(
    font_path: &std::path::Path,
    store_path: &std::path::Path,
) -> anyhow::Result<Vec<FontStylesheetInfo>> {
    splitter::split_webfont(
        None,
        ranges::WebfontDataCtx::load(),
        &std::fs::read(font_path)?,
        store_path,
    )
}
