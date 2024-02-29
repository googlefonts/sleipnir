//! Produces svgs of icons in Google-style icon fonts

use skrifa::FontRef;

use crate::error::DrawSvgError;

pub fn draw_icon(font: FontRef, codepoint: u32) -> Result<String, DrawSvgError> {
    todo!()
}

#[cfg(test)]
mod tests {
    use skrifa::FontRef;

    use crate::{icon2svg::draw_icon, testdata_bytes, testdata_string};

    static MAIL: u32 = 57688;
    static LAN: u32 = 60207;
    static MAN: u32 = 58603;

    #[test]
    fn draw_mail_icon() {
        let raw_font = testdata_bytes("vf[FILL,GRAD,opsz,wght].ttf");
        assert_eq!(
            testdata_string("mail.svg"),
            draw_icon(FontRef::new(&raw_font).unwrap(), MAIL).unwrap()
        );
    }

    #[test]
    fn draw_lan_icon() {
        let raw_font = testdata_bytes("vf[FILL,GRAD,opsz,wght].ttf");
        assert_eq!(
            testdata_string("lan.svg"),
            draw_icon(FontRef::new(&raw_font).unwrap(), LAN).unwrap()
        );
    }

    #[test]
    fn draw_man_icon() {
        let raw_font = testdata_bytes("vf[FILL,GRAD,opsz,wght].ttf");
        assert_eq!(
            testdata_string("man.svg"),
            draw_icon(FontRef::new(&raw_font).unwrap(), MAN).unwrap()
        );
    }
}
