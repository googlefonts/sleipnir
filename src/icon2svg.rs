//! Produces svgs of icons in Google-style icon fonts

use crate::{
    error::DrawSvgError, iconid::IconIdentifier, pathstyle::SvgPathStyle, pens::SvgPathPen,
};
use skrifa::{
    instance::{LocationRef, Size},
    outline::{pen::PathStyle, DrawSettings},
    raw::TableProvider,
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
                .with_path_style(PathStyle::HarfBuzz),
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
    svg.push_str(&options.style.write_svg_path(&svg_path_pen.into_inner()));
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
    style: SvgPathStyle,
}

impl<'a> DrawOptions<'a> {
    pub fn new(
        identifier: IconIdentifier,
        width_height: f32,
        location: LocationRef<'a>,
        style: SvgPathStyle,
    ) -> DrawOptions<'a> {
        DrawOptions {
            identifier,
            width_height,
            location,
            style,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        icon2svg::draw_icon,
        iconid::{self, IconIdentifier},
        pathstyle::SvgPathStyle,
        testdata,
    };
    use regex::Regex;
    use skrifa::{instance::Location, FontRef, MetadataProvider};

    use pretty_assertions::assert_eq;

    use super::DrawOptions;

    fn split_drawing_commands(svg: &str) -> Vec<String> {
        let re = Regex::new(r"([MLQCZ])").unwrap();
        re.replace_all(svg, "\n$1")
            .split('\n')
            .map(|s| s.to_string())
            .collect()
    }

    fn assert_icon_svg_equal(expected_svg: &str, actual_svg: &str) {
        assert_eq!(
            split_drawing_commands(expected_svg),
            split_drawing_commands(actual_svg),
            "Expected\n{expected_svg}\n!= Actual\n{actual_svg}",
        );
    }

    // Matches tests in code to be replaced
    fn assert_draw_icon(expected_svg: &str, identifier: IconIdentifier) {
        let font = FontRef::new(testdata::ICON_FONT).unwrap();
        let loc = font.axes().location(&[
            ("wght", 400.0),
            ("opsz", 24.0),
            ("GRAD", 0.0),
            ("FILL", 1.0),
        ]);
        let options = DrawOptions::new(identifier, 24.0, (&loc).into(), SvgPathStyle::Unchanged);

        assert_icon_svg_equal(expected_svg, &draw_icon(&font, &options).unwrap());
    }

    #[test]
    fn draw_mail_icon() {
        assert_draw_icon(testdata::MAIL_SVG, iconid::MAIL.clone());
    }

    #[test]
    fn draw_mail_icon_at_opsz48() {
        let font = FontRef::new(testdata::ICON_FONT).unwrap();
        let loc = font.axes().location(&[
            ("wght", 700.0),
            ("opsz", 48.0),
            ("GRAD", 200.0),
            ("FILL", 1.0),
        ]);
        let options = DrawOptions::new(
            iconid::MAIL.clone(),
            48.0,
            (&loc).into(),
            SvgPathStyle::Unchanged,
        );

        assert_icon_svg_equal(
            testdata::MAIL_OPSZ48_SVG,
            &draw_icon(&font, &options).unwrap(),
        );
    }

    #[test]
    fn draw_lan_icon() {
        assert_draw_icon(testdata::LAN_SVG, iconid::LAN.clone());
    }

    #[test]
    fn draw_man_icon() {
        assert_draw_icon(testdata::MAN_SVG, iconid::MAN.clone());
    }

    #[test]
    fn draw_mostly_off_curve() {
        let font = FontRef::new(testdata::MOSTLY_OFF_CURVE_FONT).unwrap();
        let loc = Location::default();
        let identifier = IconIdentifier::Codepoint(0x2e);
        let options = DrawOptions::new(identifier, 24.0, (&loc).into(), SvgPathStyle::Unchanged);

        assert_icon_svg_equal(
            testdata::MOSTLY_OFF_CURVE_SVG,
            &draw_icon(&font, &options).unwrap(),
        );
    }

    fn assert_draw_mat_symbol(expected_svg: &str, name: &str, style: SvgPathStyle) {
        let font = FontRef::new(testdata::MATERIAL_SYMBOLS_POPULAR).unwrap();
        let loc = Location::default();
        let identifier = IconIdentifier::Name(name.into());
        let options = DrawOptions::new(identifier, 24.0, (&loc).into(), style);
        let actual_svg = draw_icon(&font, &options).unwrap();
        assert_icon_svg_equal(expected_svg, &actual_svg);
    }

    // This icon was being horribly corrupted initially by compaction
    #[test]
    fn draw_info_icon_unchanged() {
        assert_draw_mat_symbol(
            testdata::INFO_UNCHANGED_SVG,
            "info",
            SvgPathStyle::Unchanged,
        );
    }

    // This icon was being horribly corrupted initially by compaction
    #[test]
    fn draw_info_icon_compact() {
        assert_draw_mat_symbol(testdata::INFO_COMPACT_SVG, "info", SvgPathStyle::Compact);
    }
}
