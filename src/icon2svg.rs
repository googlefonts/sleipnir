//! Produces svgs of icons in Google-style icon fonts
use crate::{draw_glyph::*, error::DrawSvgError};
use skrifa::{raw::TableProvider, FontRef};

pub fn draw_icon(font: &FontRef, options: &DrawOptions) -> Result<String, DrawSvgError> {
    let upem = font
        .head()
        .map_err(|e| DrawSvgError::ReadError("head", e))?
        .units_per_em();
    let viewbox = options.svg_viewbox(upem);
    let mut svg_path_pen = get_pen(viewbox, upem);
    let fill_color = options
        .fill_color
        .map(|c| format!(" fill=\"#{:08x}\"", c))
        .unwrap_or_default();

    draw_glyph(font, options, &mut svg_path_pen)?;

    let mut svg = String::with_capacity(1024);
    svg.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{} {} {} {}\" height=\"{w}\" width=\"{w}\"{fill_color}>",
        viewbox.x,
        viewbox.y,
        viewbox.width,
        viewbox.height,
        w = options.width_height.to_string(),
    ));

    // the actual path
    svg.push_str("<path d=\"");
    svg.push_str(&options.style.write_svg_path(&svg_path_pen.into_inner()));
    svg.push_str("\"/>");

    // svg ending
    svg.push_str("</svg>");

    Ok(svg)
}

#[cfg(test)]
mod tests {
    use crate::{
        assert_file_eq,
        icon2svg::draw_icon,
        iconid::{self, IconIdentifier},
        pathstyle::SvgPathStyle,
        testdata,
    };
    use regex::Regex;
    use skrifa::{prelude::LocationRef, FontRef, MetadataProvider};

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

    fn test_options(identifier: IconIdentifier) -> DrawOptions<'static> {
        DrawOptions::new(
            identifier,
            24.0,
            LocationRef::default(),
            SvgPathStyle::Unchanged(2),
        )
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
        assert_icon_svg_equal(
            expected_svg,
            &draw_icon(
                &font,
                &DrawOptions {
                    location: LocationRef::from(&loc),
                    ..test_options(identifier)
                },
            )
            .unwrap(),
        );
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
            SvgPathStyle::Unchanged(2),
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
        assert_icon_svg_equal(
            testdata::MOSTLY_OFF_CURVE_SVG,
            &draw_icon(
                &FontRef::new(testdata::MOSTLY_OFF_CURVE_FONT).unwrap(),
                &test_options(IconIdentifier::Codepoint(0x2e)),
            )
            .unwrap(),
        );
    }

    // This icon was being horribly corrupted initially by compaction
    #[test]
    fn draw_info_icon_unchanged() {
        assert_file_eq!(
            draw_icon(
                &FontRef::new(testdata::MATERIAL_SYMBOLS_POPULAR).unwrap(),
                &test_options(IconIdentifier::Name("info".into())),
            )
            .unwrap(),
            "info_unchanged.svg"
        );
    }

    // This icon was being horribly corrupted initially by compaction
    #[test]
    fn draw_info_icon_compact() {
        assert_file_eq!(
            draw_icon(
                &FontRef::new(testdata::MATERIAL_SYMBOLS_POPULAR).unwrap(),
                &DrawOptions {
                    style: SvgPathStyle::Compact(2),
                    ..test_options(IconIdentifier::Name("info".into()))
                },
            )
            .unwrap(),
            "info_compact.svg"
        );
    }

    #[test]
    fn draw_mail_icon_viewbox() {
        let font = FontRef::new(testdata::ICON_FONT).unwrap();
        let loc = font.axes().location(&[
            ("wght", 400.0),
            ("opsz", 24.0),
            ("GRAD", 0.0),
            ("FILL", 1.0),
        ]);

        assert_file_eq!(
            draw_icon(
                &font,
                &DrawOptions {
                    use_width_height_for_viewbox: true,
                    location: LocationRef::from(&loc),
                    ..test_options(iconid::MAIL.clone())
                }
            )
            .unwrap(),
            "mail_viewBox.svg"
        );
    }

    fn test_color(fill: Option<u32>, expected: Option<&str>) {
        let font = FontRef::new(testdata::ICON_FONT).unwrap();
        let loc = font.axes().location(&[
            ("wght", 400.0),
            ("opsz", 24.0),
            ("GRAD", 0.0),
            ("FILL", 1.0),
        ]);
        let mut options = DrawOptions::new(
            iconid::MAIL.clone(),
            24.0,
            (&loc).into(),
            SvgPathStyle::Unchanged(2),
        );
        options.fill_color = fill;

        let actual_svg = draw_icon(&font, &options).unwrap();
        match expected {
            Some(s) => assert!(
                actual_svg.contains(s),
                "expected '{}' in svg: {}",
                s,
                actual_svg
            ),
            None => {
                let re = Regex::new(r#"<path[^>]*fill="#).unwrap();
                assert!(
                    !re.is_match(&actual_svg),
                    "expected no fill attribute on path: {}",
                    actual_svg
                );
            }
        }
    }

    #[test]
    fn draw_mail_icon_with_fill() {
        // RRGGBBAA: red=0x11, green=0x22, blue=0x33, alpha=0xff
        test_color(Some(0x112233ff), Some("fill=\"#112233ff\""));
        test_color(Some(0xfa), Some("fill=\"#000000fa\""));
    }

    #[test]
    fn draw_mail_icon_without_fill_has_no_fill_attr() {
        test_color(None, None);
    }
}
