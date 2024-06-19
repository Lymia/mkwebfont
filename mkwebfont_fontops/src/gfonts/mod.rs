use crate::font_info::FontStyle;
use bincode::{Decode, Encode};
use mkwebfont_common::hashing::{to_nix_base32, RawHash};
use std::{
    fmt::{Debug, Formatter},
    ops::RangeInclusive,
};

#[derive(Debug, Clone, Decode, Encode)]
pub struct GfontsList {
    pub url_prefix: String,
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

#[derive(Clone, Decode, Encode)]
pub struct GfontStyleInfo {
    pub style: FontStyle,
    pub weight: RangeInclusive<u32>,
    pub url_suffix: String,
    pub hash: RawHash,
}
impl Debug for GfontStyleInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GfontStyleInfo")
            .field("style", &self.style)
            .field("weight", &self.weight)
            .field("url_suffix", &self.url_suffix)
            .field("hash", &to_nix_base32(&self.hash))
            .finish()
    }
}
