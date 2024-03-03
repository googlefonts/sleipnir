//! Produces svgs of icons in Google-style icon fonts

use crate::{error::DrawSvgError, iconid::IconIdentifier};
use skrifa::FontRef;

pub fn draw_icon(font: &FontRef, identifier: IconIdentifier) -> Result<String, DrawSvgError> {
    let gid = identifier
        .resolve(font)
        .map_err(|e| DrawSvgError::ResolutionError(identifier, e))?;
    todo!("Draw {gid}")
}

#[cfg(test)]
mod tests {
    use skrifa::FontRef;

    use crate::{icon2svg::draw_icon, iconid, testdata_bytes, testdata_string};

    #[test]
    fn draw_mail_icon() {
        let raw_font = testdata_bytes("vf[FILL,GRAD,opsz,wght].ttf");
        assert_eq!(
            testdata_string("mail.svg"),
            draw_icon(&FontRef::new(&raw_font).unwrap(), iconid::MAIL.clone()).unwrap()
        );
    }

    #[test]
    fn draw_lan_icon() {
        let raw_font = testdata_bytes("vf[FILL,GRAD,opsz,wght].ttf");
        assert_eq!(
            testdata_string("lan.svg"),
            draw_icon(&FontRef::new(&raw_font).unwrap(), iconid::LAN.clone()).unwrap()
        );
    }

    #[test]
    fn draw_man_icon() {
        let raw_font = testdata_bytes("vf[FILL,GRAD,opsz,wght].ttf");
        assert_eq!(
            testdata_string("man.svg"),
            draw_icon(&FontRef::new(&raw_font).unwrap(), iconid::MAN.clone()).unwrap()
        );
    }
}
