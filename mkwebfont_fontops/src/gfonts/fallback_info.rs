use bincode::{Decode, Encode};
use mkwebfont_common::character_set::CompressedCharacterSet;

#[derive(Debug, Clone, Decode, Encode)]
pub struct FallbackInfo {
    pub fonts: Vec<FallbackFontSource>,
}

#[derive(Debug, Clone, Decode, Encode)]
pub struct FallbackFontSource {
    pub name: String,
    pub codepoints: CompressedCharacterSet,
}
