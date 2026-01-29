pub mod cmp;
mod draw_commands;
pub mod draw_glyph;
pub mod error;
pub mod icon2kt;
pub mod icon2svg;
pub mod icon2symbol;
pub mod icon2xml;
pub mod iconid;
pub mod ligatures;
pub mod measure;
pub mod pathstyle;
mod pens;
pub mod svg_font;
#[cfg(test)]
mod test_utils;
pub mod text2png;
mod xml_element;

/// Setup to match fontations/font-test-data because that rig works for google3
#[cfg(test)]
mod testdata {
    pub static LAN_SVG: &str = include_str!("../resources/testdata/lan.svg");
    pub static MAN_SVG: &str = include_str!("../resources/testdata/man.svg");
    pub static MAIL_SVG: &str = include_str!("../resources/testdata/mail.svg");
    pub static MAIL_OPSZ48_SVG: &str = include_str!("../resources/testdata/mail_opsz48.svg");
    pub static MOSTLY_OFF_CURVE_SVG: &str =
        include_str!("../resources/testdata/mostly_off_curve.svg");

    pub static MAIL_XML: &str = include_str!("../resources/testdata/mail.xml");
    pub static MAIL_KT: &str = include_str!("../resources/testdata/mail.kt");
    pub static MAIL_VIEWBOX_XML: &str = include_str!("../resources/testdata/mail_viewBox.xml");
    pub static ICON_FONT: &[u8] =
        include_bytes!("../resources/testdata/vf[FILL,GRAD,opsz,wght].ttf");
    pub static MOSTLY_OFF_CURVE_FONT: &[u8] =
        include_bytes!("../resources/testdata/mostly_off_curve.ttf");
    pub static MATERIAL_SYMBOLS_POPULAR: &[u8] =
        include_bytes!("../resources/testdata/MaterialSymbolsOutlinedVF-Popular.ttf");
    pub static LIGA_TESTS_FONT: &[u8] = include_bytes!("../resources/testdata/liga_test.otf");

    pub static FULL_VF_OLD: &[u8] = include_bytes!("../resources/testdata/large_vf_old.ttf");
    pub static FULL_VF_NEW: &[u8] = include_bytes!("../resources/testdata/large_vf_new.ttf");

    pub static PLAY_ARROW_VF: &[u8] = include_bytes!("../resources/testdata/play_arrow_vf.ttf");

    // Only includes ABab.
    pub static NABLA_FONT: &[u8] = include_bytes!("../resources/testdata/nabla.ttf");
    // Generated with:
    //   klippa --path NotoColorEmoji-Regular.ttf --output-file resources/testdata/NotoColorEmoji.ttf \
    //          --unicodes U+1F973 --gids 1760
    pub static NOTO_EMOJI_FONT: &[u8] = include_bytes!("../resources/testdata/NotoColorEmoji.ttf");
    pub static CAVEAT_FONT: &[u8] = include_bytes!("../resources/testdata/caveat.ttf");
    pub static NOTO_KUFI_ARABIC_FONT: &[u8] =
        include_bytes!("../resources/testdata/NotoKufiArabic[wght].ttf");
    pub static NOTO_KUF_ARABIC_SVG: &str =
        include_str!("../resources/testdata/NotoKufiArabic[wght].svg");
}
