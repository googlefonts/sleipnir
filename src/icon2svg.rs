//! Produces svgs of icons in Google-style icon fonts
use std::collections::HashMap;

use crate::{
    draw_glyph::*,
    error::DrawSvgError,
    pathstyle::SvgPathStyle,
    pens::{ColorFill, ColorStop, GlyphPainter, Paint},
    xml_element::{HexColor, TruncatedFloat, XmlElement},
};
use kurbo::Affine;
use skrifa::{prelude::Size, raw::TableProvider, FontRef, GlyphId, MetadataProvider};
use tiny_skia::Color;

pub fn draw_icon(font: &FontRef, options: &DrawOptions) -> Result<String, DrawSvgError> {
    let gid = options
        .identifier
        .resolve(font, &options.location)
        .map_err(|e| DrawSvgError::ResolutionError(options.identifier.clone(), e))?;

    let upem = font
        .head()
        .map_err(|e| DrawSvgError::ReadError("head", e))?
        .units_per_em();
    if let Some(glyph) = font.color_glyphs().get(gid) {
        return draw_color_glyph(font, glyph, gid, options, upem);
    }

    let viewbox = options.svg_viewbox(None, upem);
    let mut svg_path_pen = get_pen(viewbox, upem);
    draw_glyph(font, gid, options, &mut svg_path_pen)?;

    let mut svg = XmlElement::new("svg")
        .with_attribute("xmlns", "http://www.w3.org/2000/svg")
        .with_attribute(
            "viewBox",
            format!(
                "{} {} {} {}",
                viewbox.x, viewbox.y, viewbox.width, viewbox.height
            ),
        )
        .with_attribute("height", options.width_height)
        .with_attribute("width", options.width_height);

    if let Some(c) = options.fill_color {
        svg.add_attribute("fill", format!("#{:08x}", c));
    }

    Ok(svg
        .with_child(XmlElement::new("path").with_attribute(
            "d",
            options.style.write_svg_path(&svg_path_pen.into_inner()),
        ))
        .to_string())
}

pub(crate) fn color_from_u32(c: u32) -> Color {
    let [r, g, b, a] = c.to_be_bytes();
    Color::from_rgba8(r, g, b, a)
}

fn draw_color_glyph(
    font: &FontRef,
    glyph: skrifa::color::ColorGlyph,
    glyph_id: GlyphId,
    options: &DrawOptions,
    upem: u16,
) -> Result<String, DrawSvgError> {
    let viewbox = options.svg_viewbox(glyph.bounding_box(options.location, Size::unscaled()), upem);

    let foreground = options
        .fill_color
        .map(color_from_u32)
        .unwrap_or(Color::BLACK);

    let mut painter = GlyphPainter::new(font, options.location, foreground, Size::unscaled());
    if let Err(e) = glyph.paint(options.location, &mut painter) {
        return Err(DrawSvgError::PaintError(
            options.identifier.clone(),
            glyph_id,
            e,
        ));
    }

    let svg = XmlElement::new("svg")
        .with_attribute("xmlns", "http://www.w3.org/2000/svg")
        .with_attribute(
            "viewBox",
            format!(
                "{} {} {} {}",
                viewbox.x, viewbox.y, viewbox.width, viewbox.height,
            ),
        )
        .with_attribute("height", options.width_height)
        .with_attribute("width", options.width_height)
        .with_child(to_svg(painter.into_fills()?, &options.style));

    Ok(svg.to_string())
}

fn to_svg(fills: Vec<ColorFill>, style: &SvgPathStyle) -> XmlElement {
    let mut group = Vec::new();

    let mut clip_defs = Vec::new();
    let mut clip_cache = HashMap::<(Option<ClipId>, String), ClipId>::new();
    let mut fill_idx_to_clip_id = HashMap::<usize, ClipId>::new();

    // Pass 1: Generate clip defs
    for (i, fill) in fills.iter().enumerate() {
        if fill.clip_paths.len() > 1 {
            let clips = &fill.clip_paths[0..fill.clip_paths.len() - 1];
            let mut parent_id = None;
            for clip in clips {
                let key = (parent_id, style.write_svg_path(clip).to_string());
                let clip_id = match clip_cache.get(&key) {
                    Some(id) => *id,
                    None => {
                        let new_id = ClipId(clip_cache.len());
                        let mut clip_path = XmlElement::new("clipPath")
                            .with_attribute("id", new_id)
                            .with_child(XmlElement::new("path").with_attribute("d", key.1.clone()));
                        if let Some(pid) = parent_id {
                            clip_path.add_attribute("clip-path", format!("url(#{})", pid));
                        }
                        clip_defs.push(clip_path);
                        clip_cache.insert(key, new_id);
                        new_id
                    }
                };
                parent_id = Some(clip_id);
            }
            if let Some(pid) = parent_id {
                fill_idx_to_clip_id.insert(i, pid);
            }
        }
    }

    // Pass 2: Paths
    let mut fill_cache = FillCache::default();
    for (i, fill) in fills.iter().enumerate() {
        let Some(shape) = fill.clip_paths.last() else {
            continue;
        };
        let mut path = XmlElement::new("path").with_attribute("d", style.write_svg_path(shape));
        fill_cache.add_fill(&mut path, &fill.paint);
        if let Some(id) = fill_idx_to_clip_id.get(&i) {
            path.add_attribute("clip-path", format!("url(#{})", id));
        }

        // Transform (offset)
        if fill.offset_x != 0.0 || fill.offset_y != 0.0 {
            path.add_attribute(
                "transform",
                format!("translate({} {})", fill.offset_x, fill.offset_y),
            );
        }

        group.push(path);
    }

    if !fill_cache.is_empty() || !clip_defs.is_empty() {
        group.push(
            XmlElement::new("defs")
                .with_children(clip_defs)
                .with_children(fill_cache.into_svg()),
        );
    }

    match group.len() {
        1 => group.into_iter().next().unwrap(),
        _ => XmlElement::new("g").with_children(group),
    }
}

fn affine_to_svg_matrix(affine: Affine) -> Option<String> {
    let c = affine.as_coeffs();
    match c {
        [1.0, 0.0, 0.0, 1.0, 0.0, 0.0] => None,
        [x, 0.0, 0.0, y, 0.0, 0.0] => Some(format!(
            "scale({} {})",
            TruncatedFloat(x),
            TruncatedFloat(y)
        )),
        [1.0, 0.0, 1.0, 0.0, x, y] => Some(format!(
            "translate({} {})",
            TruncatedFloat(x),
            TruncatedFloat(y)
        )),
        _ => Some(format!(
            "matrix({} {} {} {} {} {})",
            TruncatedFloat(c[0]),
            TruncatedFloat(c[1]),
            TruncatedFloat(c[2]),
            TruncatedFloat(c[3]),
            TruncatedFloat(c[4]),
            TruncatedFloat(c[5])
        )),
    }
}

fn add_stops(grad: &mut XmlElement, stops: &[ColorStop]) {
    for stop in stops {
        let mut s = XmlElement::new("stop").with_attribute("offset", stop.offset);

        s.add_attribute("stop-color", HexColor::from(stop.color).opaque());
        if !stop.color.is_opaque() {
            s.add_attribute("stop-opacity", TruncatedFloat::from(stop.color.alpha()));
        }
        grad.add_child(s);
    }
}

fn set_spread_method(grad: &mut XmlElement, extend: skrifa::color::Extend) {
    match extend {
        skrifa::color::Extend::Pad => {} // Pad is the SVG default
        skrifa::color::Extend::Repeat => grad.add_attribute("spreadMethod", "repeat"),
        skrifa::color::Extend::Reflect => grad.add_attribute("spreadMethod", "reflect"),
        // Non-exhaustive matching is required, but we should handle any variants as soon as we
        // become aware of them.
        _ => {}
    };
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PaintId(usize);

impl std::fmt::Display for PaintId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "p{}", self.0)
    }
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ClipId(usize);

impl std::fmt::Display for ClipId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "c{}", self.0)
    }
}

#[derive(Default)]
struct FillCache {
    paint_to_id: HashMap<XmlElement, PaintId>,
}

impl FillCache {
    fn into_svg(self) -> impl Iterator<Item = XmlElement> {
        let mut paints: Vec<_> = self.paint_to_id.into_iter().collect();
        paints.sort_unstable_by_key(|(_, id)| *id);
        paints
            .into_iter()
            .map(|(grad, id)| grad.with_attribute("id", id))
    }

    fn is_empty(&self) -> bool {
        self.paint_to_id.is_empty()
    }

    fn add_fill(&mut self, path: &mut XmlElement, paint: &Paint) {
        match paint {
            Paint::Solid(c) => path.add_attribute("fill", HexColor::from(*c)),
            Paint::LinearGradient {
                p0,
                p1,
                stops,
                extend,
                transform,
            } => {
                let mut grad = XmlElement::new("linearGradient")
                    .with_attribute("gradientUnits", "userSpaceOnUse")
                    .with_attribute("x1", TruncatedFloat(p0.x))
                    .with_attribute("y1", TruncatedFloat(p0.y))
                    .with_attribute("x2", TruncatedFloat(p1.x))
                    .with_attribute("y2", TruncatedFloat(p1.y));
                if let Some(t) = affine_to_svg_matrix(*transform) {
                    grad.add_attribute("gradientTransform", t);
                }
                add_stops(&mut grad, stops);
                set_spread_method(&mut grad, *extend);
                let next_id = PaintId(self.paint_to_id.len());
                let id = self.paint_to_id.entry(grad).or_insert(next_id);
                path.add_attribute("fill", format!("url(#{id})"));
            }
            Paint::RadialGradient {
                c0,
                c1,
                r0,
                r1,
                stops,
                extend,
                transform,
            } => {
                let mut grad = XmlElement::new("radialGradient")
                    .with_attribute("gradientUnits", "userSpaceOnUse")
                    .with_attribute("cx", TruncatedFloat(c1.x))
                    .with_attribute("cy", TruncatedFloat(c1.y))
                    .with_attribute("r", TruncatedFloat::from(*r1));
                if *r0 > 0.0 {
                    grad.add_attribute("fr", TruncatedFloat::from(*r0));
                }
                if c0.x != c1.x || c0.y != c1.y {
                    grad.add_attribute("fx", TruncatedFloat(c0.x));
                    grad.add_attribute("fy", TruncatedFloat(c0.y));
                }
                if let Some(t) = affine_to_svg_matrix(*transform) {
                    grad.add_attribute("gradientTransform", t);
                }
                add_stops(&mut grad, stops);
                set_spread_method(&mut grad, *extend);

                let next_id = PaintId(self.paint_to_id.len());
                let id = self.paint_to_id.entry(grad).or_insert(next_id);
                path.add_attribute("fill", format!("url(#{id})"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        assert_file_eq,
        icon2svg::{color_from_u32, draw_icon},
        iconid::{self, IconIdentifier},
        pathstyle::SvgPathStyle,
        testdata,
    };
    use regex::Regex;
    use skrifa::{prelude::LocationRef, FontRef, MetadataProvider};
    use tiny_skia::Color;

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

    fn test_options<'a>(
        identifier: IconIdentifier,
        location: impl Into<LocationRef<'a>>,
    ) -> DrawOptions<'a> {
        DrawOptions::new(
            identifier,
            24.0,
            location.into(),
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
            &draw_icon(&font, &test_options(identifier, &loc)).unwrap(),
        );
    }

    #[test]
    fn color_conversion() {
        let color = u32::from_str_radix("11223344", 16).unwrap();
        assert_eq!(color_from_u32(color), Color::from_rgba8(17, 34, 51, 68));
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

        assert_icon_svg_equal(
            testdata::MAIL_OPSZ48_SVG,
            &draw_icon(
                &font,
                &DrawOptions {
                    width_height: 48.0,
                    ..test_options(iconid::MAIL.clone(), &loc)
                },
            )
            .unwrap(),
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
                &test_options(IconIdentifier::Codepoint(0x2e), LocationRef::default()),
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
                &test_options(IconIdentifier::Name("info".into()), LocationRef::default()),
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
                    ..test_options(IconIdentifier::Name("info".into()), LocationRef::default())
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
                    ..test_options(iconid::MAIL.clone(), &loc)
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
        let options = DrawOptions {
            fill_color: fill,
            ..test_options(iconid::MAIL.clone(), &loc)
        };

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

    #[test]
    fn draw_color_icon() {
        let font = FontRef::new(testdata::NOTO_EMOJI_FONT).unwrap();
        let svg = draw_icon(
            &font,
            &DrawOptions::new(
                IconIdentifier::Codepoint('🥳' as u32),
                128.0,
                LocationRef::default(),
                SvgPathStyle::Unchanged(2),
            ),
        )
        .unwrap();
        assert_file_eq!(svg, "color_icon.svg");
    }
}
