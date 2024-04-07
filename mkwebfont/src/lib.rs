mod contrib;
mod fonts;
mod ranges;
mod splitter;

pub use fonts::{FontStyle, FontWeight};
pub use splitter::{FontStylesheetEntry, FontStylesheetInfo};

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct SplitWebfontCtx {
    pub splitter_tuning: Option<String>,
    pub preload_codepoints: roaring::RoaringBitmap,
}
impl Default for SplitWebfontCtx {
    fn default() -> Self {
        SplitWebfontCtx { splitter_tuning: None, preload_codepoints: Default::default() }
    }
}

pub fn split_webfont(
    split_ctx: &SplitWebfontCtx,
    font_path: &std::path::Path,
    store_path: &std::path::Path,
) -> anyhow::Result<Vec<FontStylesheetInfo>> {
    splitter::split_webfont(
        split_ctx,
        None,
        ranges::WebfontDataCtx::load(),
        &std::fs::read(font_path)?,
        store_path,
    )
}
