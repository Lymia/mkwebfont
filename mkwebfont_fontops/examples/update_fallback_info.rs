use anyhow::Result;
use mkwebfont_common::{
    character_set::CharacterSet, compression::zstd_compress, download_cache::DownloadInfo,
};
use mkwebfont_fontops::{
    font_info::FontFaceWrapper,
    gfonts::{
        fallback_info::{FallbackComponent, FallbackDownloadSource, FallbackInfo},
        gfonts_list::GfontsList,
    },
};
use std::{io, path::PathBuf};
use tracing::{error, info};
use unicode_properties::{GeneralCategoryGroup, UnicodeGeneralCategory};

const FALLBACK_FONTS: &[&str] = &[
    "Noto Sans",
    // Symbol Fonts
    "Noto Sans Math",
    "Noto Music",
    "Noto Sans Symbols",
    "Noto Sans Symbols 2",
    // CJK
    "Noto Sans JP",
    "Noto Sans SC",
    "Noto Sans TC",
    "Noto Sans KR",
    // Emoji Fonts
    "Noto Color Emoji",
    "Noto Emoji",
    // Other languages
    "Noto Sans Adlam",
    "Noto Serif Ahom",
    "Noto Sans Anatolian Hieroglyphs",
    "Noto Sans Arabic",
    "Noto Sans Armenian",
    "Noto Sans Avestan",
    "Noto Sans Balinese",
    "Noto Sans Bamum",
    "Noto Sans Bassa Vah",
    "Noto Sans Batak",
    "Noto Sans Bengali",
    "Noto Sans Bhaiksuki",
    "Noto Sans Brahmi",
    "Noto Sans Buginese",
    "Noto Sans Buhid",
    "Noto Sans Canadian Aboriginal",
    "Noto Sans Carian",
    "Noto Sans Caucasian Albanian",
    "Noto Sans Chakma",
    "Noto Sans Cham",
    "Noto Sans Cherokee",
    "Noto Sans Chorasmian",
    "Noto Sans Coptic",
    "Noto Sans Cuneiform",
    "Noto Sans Cypriot",
    "Noto Sans Cypro Minoan",
    "Noto Sans Deseret",
    "Noto Sans Devanagari",
    //"Noto Serif Dives Akuru", // TODO: Not on Google Fonts
    "Noto Serif Dogra",
    "Noto Sans Duployan",
    "Noto Sans Egyptian Hieroglyphs",
    "Noto Sans Elbasan",
    "Noto Sans Elymaic",
    "Noto Sans Ethiopic",
    "Noto Sans Georgian",
    "Noto Sans Glagolitic",
    "Noto Sans Gothic",
    "Noto Sans Grantha",
    "Noto Sans Gujarati",
    "Noto Sans Gunjala Gondi",
    "Noto Sans Gurmukhi",
    "Noto Sans Hanifi Rohingya",
    "Noto Sans Hanunoo",
    "Noto Sans Hatran",
    "Noto Sans Hebrew",
    "Noto Sans Imperial Aramaic",
    "Noto Sans Indic Siyaq Numbers",
    "Noto Sans Inscriptional Pahlavi",
    "Noto Sans Inscriptional Parthian",
    "Noto Sans Javanese",
    "Noto Sans Kaithi",
    "Noto Sans Kannada",
    "Noto Sans Kawi",
    "Noto Sans Kayah Li",
    "Noto Sans Kharoshthi",
    "Noto Serif Khitan Small Script",
    "Noto Sans Khmer",
    "Noto Sans Khojki",
    "Noto Sans Khudawadi",
    "Noto Sans Lao",
    "Noto Sans Lao Looped",
    "Noto Sans Lepcha",
    "Noto Sans Limbu",
    "Noto Sans Linear A",
    "Noto Sans Linear B",
    "Noto Sans Lisu",
    "Noto Sans Lycian",
    "Noto Sans Lydian",
    "Noto Sans Mahajani",
    "Noto Serif Makasar",
    "Noto Sans Malayalam",
    "Noto Sans Mandaic",
    "Noto Sans Manichaean",
    "Noto Sans Marchen",
    "Noto Sans Masaram Gondi",
    "Noto Sans Medefaidrin",
    "Noto Sans Meetei Mayek",
    "Noto Sans Mende Kikakui",
    "Noto Sans Meroitic",
    "Noto Sans Miao",
    "Noto Sans Modi",
    "Noto Sans Mongolian",
    "Noto Sans Mro",
    "Noto Sans Multani",
    "Noto Sans Myanmar",
    "Noto Sans Nabataean",
    "Noto Sans Nag Mundari",
    "Noto Sans Nandinagari",
    "Noto Sans New Tai Lue",
    "Noto Sans Newa",
    "Noto Sans Nko",
    "Noto Traditional Nushu",
    "Noto Serif Hmong Nyiakeng",
    "Noto Sans Ogham",
    "Noto Sans Ol Chiki",
    "Noto Sans Old Hungarian",
    "Noto Sans Old Italic",
    "Noto Sans Old North Arabian",
    "Noto Sans Old Permic",
    "Noto Sans Old Persian",
    "Noto Sans Old Sogdian",
    "Noto Sans Old South Arabian",
    "Noto Sans Old Turkic",
    "Noto Serif Old Uyghur",
    "Noto Sans Oriya",
    "Noto Sans Osage",
    "Noto Sans Osmanya",
    "Noto Serif Ottoman Siyaq",
    "Noto Sans Pahawh Hmong",
    "Noto Sans Palmyrene",
    "Noto Sans Pau Cin Hau",
    "Noto Sans PhagsPa",
    "Noto Sans Phoenician",
    "Noto Sans Psalter Pahlavi",
    "Noto Sans Rejang",
    "Noto Sans Runic",
    "Noto Sans Samaritan",
    "Noto Sans Saurashtra",
    "Noto Sans Sharada",
    "Noto Sans Shavian",
    "Noto Sans Siddham",
    "Noto Sans SignWriting",
    "Noto Sans Sinhala",
    "Noto Sans Sogdian",
    "Noto Sans Sora Sompeng",
    "Noto Sans Soyombo",
    "Noto Sans Sundanese",
    "Noto Sans Syloti Nagri",
    "Noto Sans Syriac",
    "Noto Sans Tagalog",
    "Noto Sans Tagbanwa",
    "Noto Sans Tai Le",
    "Noto Sans Tai Tham",
    "Noto Sans Tai Viet",
    "Noto Sans Takri",
    "Noto Sans Tamil",
    "Noto Sans Tamil Supplement",
    "Noto Sans Tangsa",
    "Noto Serif Tangut",
    "Noto Sans Telugu",
    "Noto Sans Thaana",
    "Noto Sans Thai",
    "Noto Serif Tibetan",
    "Noto Sans Tifinagh",
    "Noto Sans Tirhuta",
    "Noto Serif Toto",
    "Noto Sans Ugaritic",
    "Noto Sans Vai",
    "Noto Sans Vithkuqi",
    "Noto Sans Wancho",
    "Noto Sans Warang Citi",
    "Noto Serif Yezidi",
    "Noto Sans Yi",
    "Noto Sans Zanabazar Square",
    // Misc Noto Fonts
    "Noto Znamenny Musical Notation",
];
const EXTRA_FONTS_NAMES: &[&[&str]] = &[
    &["NotoSerifDivesAkuru-Regular.ttf"],
    &["KurintoSans-Rg.ttf"],
    &["KurintoSansCJK-Rg.ttf"],
    &["BabelStoneHan.ttf"],
];

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(io::stderr)
        .init();

    let Some(repo_path) = std::env::args().skip(1).next() else {
        error!("Pass the path of the directory with the fallback fonts.");
        return Ok(());
    };
    let Some(uri_prefix) = std::env::args().skip(2).next() else {
        error!("Pass the path of the directory with the URI prefix.");
        return Ok(());
    };

    let mut all_chars = CharacterSet::new();
    for i in 0..0x110000 {
        if let Some(ch) = char::from_u32(i) {
            if let Some(_) = unicode_blocks::find_unicode_block(ch) {
                if ch.general_category_group() != GeneralCategoryGroup::Other {
                    all_chars.insert(i);
                }
            }
        }
    }

    let mut fonts = Vec::new();
    macro_rules! gfont {
        ($name:expr) => {{
            let font = GfontsList::find_font($name).unwrap();

            let mut loaded = Vec::new();
            for style in &font.styles {
                loaded.extend(FontFaceWrapper::load(None, style.info.load().await?)?);
            }
            let source = FallbackDownloadSource::GFonts($name.to_string());
            fonts.push((source, loaded));
        }};
    }
    for font_name in FALLBACK_FONTS {
        gfont!(*font_name);
    }
    for extra_font in EXTRA_FONTS_NAMES {
        let mut loaded = Vec::new();
        let mut downloads = Vec::new();
        for style in *extra_font {
            let path = format!("{repo_path}/{style}");
            loaded.extend(FontFaceWrapper::load(None, std::fs::read(&path)?)?);

            let uri = format!("{uri_prefix}/{style}");
            downloads.push(DownloadInfo::for_file(&PathBuf::from(path), &uri)?);
        }
        fonts.push((FallbackDownloadSource::Download(downloads), loaded));
    }
    gfont!("Adobe Blank");

    let mut fallback_info = FallbackInfo { fonts: vec![] };
    for (source, loaded) in fonts {
        let font_name = loaded[0].font_family();
        let mut available = loaded[0].all_codepoints().clone();
        for font in &loaded[1..] {
            available &= font.all_codepoints();
        }

        let fulfilled = &all_chars & &available;
        all_chars -= &fulfilled;

        info!(
            "{font_name:40}: {} available / {} fulfilled / {} remaining",
            available.len(),
            fulfilled.len(),
            all_chars.len(),
        );

        fallback_info.fonts.push(FallbackComponent {
            name: font_name.to_string(),
            source,
            codepoints: fulfilled.compressed(),
        })
    }

    println!("{fallback_info:#?}");

    std::fs::write(
        "mkwebfont_fontops/src/gfonts/fallback_info.bin.zst",
        zstd_compress(&bincode::encode_to_vec(&fallback_info, bincode::config::standard())?)?,
    )?;

    Ok(())
}
