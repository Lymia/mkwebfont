use crate::font_info::FontStyle;
use bincode::{Decode, Encode};
use mkwebfont_common::download_cache::DownloadInfo;
use std::{fmt::Debug, ops::RangeInclusive};

#[derive(Debug, Clone, Decode, Encode)]
pub struct GfontsList {
    pub repo_revision: String,
    pub repo_date: String,
    pub repo_short_date: String,
    pub fonts: Vec<GfontInfo>,
}

#[derive(Debug, Clone, Decode, Encode)]
pub struct GfontInfo {
    pub name: String,
    pub styles: Vec<GfontStyleInfo>,
}

#[derive(Debug, Clone, Decode, Encode)]
pub struct GfontStyleInfo {
    pub style: FontStyle,
    pub weight: RangeInclusive<u32>,
    pub info: DownloadInfo,
}
