//! Produces svgs of icons in Google-style icon fonts

use crate::{error::DrawSvgError, iconid::IconIdentifier, pens::SvgPathPen};
use skrifa::{
    instance::{LocationRef, Size},
    outline::DrawSettings,
    raw::{tables::glyf::ToPathStyle, TableProvider},
    FontRef, MetadataProvider,
};

pub fn draw_icon(font: &FontRef, options: &DrawOptions<'_>) -> Result<String, DrawSvgError> {
    let upem = font
        .head()
        .map_err(|e| DrawSvgError::ReadError("head", e))?
        .units_per_em();
    let gid = options
        .identifier
        .resolve(font, &options.location)
        .map_err(|e| DrawSvgError::ResolutionError(options.identifier.clone(), e))?;

    let glyph = font
        .outline_glyphs()
        .get(gid)
        .ok_or(DrawSvgError::NoOutline(options.identifier.clone(), gid))?;

    // Draw the glyph. Fonts are Y-up, svg Y-down so flip-y.
    let mut svg_path_pen = SvgPathPen::new();

    glyph
        .draw(
            DrawSettings::unhinted(Size::unscaled(), options.location)
                .with_path_style(ToPathStyle::HarfBuzz),
            &mut svg_path_pen,
        )
        .map_err(|e| DrawSvgError::DrawError(options.identifier.clone(), gid, e))?;

    let upem_str = upem.to_string();
    let width_height = options.width_height.to_string();
    let mut svg = String::with_capacity(1024);
    // svg preamble
    // This viewBox matches existing code we are moving to Rust
    svg.push_str("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 -");
    svg.push_str(&upem_str);
    svg.push(' ');
    svg.push_str(&upem_str);
    svg.push(' ');
    svg.push_str(upem_str.as_str());
    svg.push_str("\" height=\"");
    svg.push_str(&width_height);
    svg.push_str("\" width=\"");
    svg.push_str(&width_height);
    svg.push_str("\">");

    // the actual path
    svg.push_str("<path d=\"");
    svg.push_str(&svg_path_pen.to_svg_path());
    //svg.push_str(&path_pen.into_inner().to_svg());
    svg.push_str("\"/>");

    // svg ending
    svg.push_str("</svg>");

    Ok(svg)
}

pub struct DrawOptions<'a> {
    identifier: IconIdentifier,
    width_height: f32,
    location: LocationRef<'a>,
}

impl<'a> DrawOptions<'a> {
    pub fn new(
        identifier: IconIdentifier,
        width_height: f32,
        location: LocationRef<'a>,
    ) -> DrawOptions<'a> {
        DrawOptions {
            identifier,
            width_height,
            location,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        icon2svg::draw_icon,
        iconid::{self, IconIdentifier},
        testdata_bytes, testdata_string,
    };
    use skrifa::{instance::Location, FontRef, MetadataProvider};

    use super::DrawOptions;

    // Matches tests in code to be replaced
    fn assert_draw_icon(expected_file: &str, identifier: IconIdentifier) {
        let raw_font = testdata_bytes("vf[FILL,GRAD,opsz,wght].ttf");
        let font = FontRef::new(&raw_font).unwrap();
        let loc = font.axes().location(&[
            ("wght", 400.0),
            ("opsz", 24.0),
            ("GRAD", 0.0),
            ("FILL", 1.0),
        ]);
        let options = DrawOptions::new(identifier, 24.0, (&loc).into());

        assert_eq!(
            testdata_string(expected_file),
            draw_icon(&font, &options).unwrap()
        );
    }

    #[test]
    fn draw_mail_icon() {
        assert_draw_icon("mail.svg", iconid::MAIL.clone());
    }

    #[test]
    fn draw_lan_icon() {
        assert_draw_icon("lan.svg", iconid::LAN.clone());
    }

    #[test]
    fn draw_man_icon() {
        assert_draw_icon("man.svg", iconid::MAN.clone());
    }

    #[test]
    fn draw_mostly_off_curve() {
        let raw_font = testdata_bytes("mostly_off_curve.ttf");
        let font = FontRef::new(&raw_font).unwrap();
        let loc = Location::default();
        let identifier = IconIdentifier::Codepoint(0x2e);
        let options = DrawOptions::new(identifier, 24.0, (&loc).into());

        assert_eq!(
            testdata_string("mostly_off_curve.svg"),
            draw_icon(&font, &options).unwrap()
        );
    }
}
