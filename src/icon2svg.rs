//! Produces svgs of icons in Google-style icon fonts

use crate::{error::DrawSvgError, iconid::IconIdentifier};
use kurbo::{Affine, BezPath, PathEl, Point};
use skrifa::{
    instance::{LocationRef, Size},
    outline::DrawSettings,
    raw::TableProvider,
    FontRef, MetadataProvider,
};
use std::fmt::Write;
use write_fonts::pens::{BezPathPen, TransformPen};

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn push_point(svg: &mut String, prefix: char, p: Point) {
    svg.push(prefix);
    write!(svg, "{},{}", round2(p.x), round2(p.y)).expect("We can't write into a String?!");
}

/// We use this rather than [`BezPath::to_svg`]` so we can exactly match the output of the tool we seek to replace
fn push_drawing_commands(svg: &mut String, path: &BezPath) {
    for el in path.elements() {
        match el {
            PathEl::MoveTo(p) => push_point(svg, 'M', *p),
            PathEl::LineTo(p) => push_point(svg, 'L', *p),
            PathEl::QuadTo(p1, p2) => {
                push_point(svg, 'Q', *p1);
                push_point(svg, ' ', *p2);
            }
            PathEl::CurveTo(p1, p2, p3) => {
                push_point(svg, 'C', *p1);
                push_point(svg, ' ', *p2);
                push_point(svg, ' ', *p3);
            }
            PathEl::ClosePath => svg.push('Z'),
        }
    }
}

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
    let mut path_pen = BezPathPen::new();
    let mut transform_pen = TransformPen::new(&mut path_pen, Affine::FLIP_Y);

    glyph
        .draw(
            DrawSettings::unhinted(Size::unscaled(), options.location),
            &mut transform_pen,
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
    push_drawing_commands(&mut svg, &path_pen.into_inner());
    svg.push_str("\">");

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
    use pretty_assertions::assert_eq;
    use skrifa::{FontRef, MetadataProvider};

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
    #[ignore] // until matching svgs is fixed
    fn draw_mail_icon() {
        assert_draw_icon("mail.svg", iconid::MAIL.clone());
    }

    #[test]
    #[ignore] // until matching svgs is fixed
    fn draw_lan_icon() {
        assert_draw_icon("lan.svg", iconid::LAN.clone());
    }

    #[test]
    #[ignore] // until matching svgs is fixed
    fn draw_man_icon() {
        assert_draw_icon("man.svg", iconid::MAN.clone());
    }
}
