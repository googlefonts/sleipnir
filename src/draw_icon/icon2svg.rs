//! Produces svgs of icons in Google-style icon fonts
use std::collections::HashMap;

use super::{draw_glyph, get_pen, DrawOptions, DrawingInstructions, GlyphType};
use crate::{
    error::DrawSvgError,
    pathstyle::SvgPathStyle,
    pens::{ColorFill, ColorStop, GlyphPainter, Paint},
    xml_element::{HexColor, TruncatedFloat, XmlElement},
};
use kurbo::Affine;
use skrifa::{prelude::Size, FontRef, GlyphId};
use tiny_skia::Color;

/// Draws an icon from a font.
///
/// This function supports both simple glyphs and color glyphs (COLR).
pub(super) fn draw_svg(
    font: &FontRef,
    di: DrawingInstructions,
    options: &DrawOptions,
) -> Result<String, DrawSvgError> {
    let mut svg = XmlElement::new("svg")
        .with_attribute("xmlns", "http://www.w3.org/2000/svg")
        .with_attribute(
            "viewBox",
            format!(
                "{} {} {} {}",
                di.viewbox.x, di.viewbox.y, di.viewbox.width, di.viewbox.height
            ),
        )
        .with_attribute("height", options.height)
        .with_attribute("width", di.glyph_width);
    match di.glyph {
        GlyphType::Color(glyph) => {
            svg = svg.with_child(draw_color_glyph(font, glyph, di.glyph_id, options)?)
        }
        GlyphType::Outline(glyph) => {
            let mut svg_path_pen = get_pen(di.viewbox, di.upem);
            draw_glyph(glyph, options, &mut svg_path_pen)?;
            svg = svg.with_child(XmlElement::new("path").with_attribute(
                "d",
                options.style.write_svg_path(&svg_path_pen.into_inner()),
            ));
            if let Some(c) = options.fill_color {
                svg.add_attribute("fill", format!("#{:08x}", c));
            }
        }
    };

    Ok(svg.to_string())
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
) -> Result<XmlElement, DrawSvgError> {
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

    to_svg(painter.into_fills()?, &options.style)
}

fn to_svg(fills: Vec<ColorFill>, style: &SvgPathStyle) -> Result<XmlElement, DrawSvgError> {
    let mut group = Vec::new();
    let mut clips_cache = ClipsCache::default();
    let mut fill_cache = PaintCache::default();
    for fill in fills.iter() {
        // Path
        let Some(shape) = fill.clip_paths.last() else {
            continue;
        };
        let mut path = XmlElement::new("path").with_attribute("d", style.write_svg_path(shape));

        // Fill
        fill_cache.add_fill(&mut path, &fill.paint)?;

        // Clip
        let mut clip_parent_id = None;
        if fill.clip_paths.len() > 1 {
            for clip in &fill.clip_paths[0..fill.clip_paths.len() - 1] {
                let id = clips_cache.get_id(clip_parent_id, style.write_svg_path(clip).to_string());
                clip_parent_id = Some(id);
            }
        }
        if let Some(id) = clip_parent_id {
            path.add_attribute("clip-path", format!("url(#{})", id));
        }

        // Offset
        if fill.offset_x != 0.0 || fill.offset_y != 0.0 {
            path.add_attribute(
                "transform",
                format!("translate({} {})", fill.offset_x, fill.offset_y),
            );
        }

        group.push(path);
    }

    if !fill_cache.is_empty() || !clips_cache.is_empty() {
        group.push(
            XmlElement::new("defs")
                .with_children(clips_cache.into_svg())
                .with_children(fill_cache.into_svg()),
        );
    }

    let xml = match group.len() {
        1 => group.into_iter().next().unwrap(),
        _ => XmlElement::new("g").with_children(group),
    };
    Ok(xml)
}

/// Caches and manages SVG clip paths to avoid duplicates in the `<defs>` section.
#[derive(Default)]
struct ClipsCache {
    // Key is (parent_clip_id, path_d)
    path_with_parent_to_id: HashMap<(Option<ClipId>, String), ClipId>,
}

impl ClipsCache {
    /// Get the id for a clip with the given parent and path.
    fn get_id(&mut self, parent_id: Option<ClipId>, path_d: String) -> ClipId {
        let next_id = ClipId(self.path_with_parent_to_id.len());
        *self
            .path_with_parent_to_id
            .entry((parent_id, path_d.clone()))
            .or_insert(next_id)
    }

    /// Returns an iterator over the clip elements, suitable for inclusion in `<defs>`.
    fn into_svg(self) -> impl Iterator<Item = XmlElement> {
        let mut clips: Vec<_> = self.path_with_parent_to_id.into_iter().collect();
        clips.sort_unstable_by_key(|(_, id)| *id);
        clips.into_iter().map(|((parent_id, path), id)| {
            let mut clip = XmlElement::new("clipPath")
                .with_attribute("id", id)
                .with_child(XmlElement::new("path").with_attribute("d", path));
            if let Some(id) = parent_id {
                clip.add_attribute("clip-path", format!("url(#{})", id));
            }
            clip
        })
    }

    /// Returns true if there are no clips.
    fn is_empty(&self) -> bool {
        self.path_with_parent_to_id.is_empty()
    }
}

/// Caches and manages SVG paints (gradients) to avoid duplicates in the `<defs>` section.
#[derive(Default)]
struct PaintCache {
    paint_to_id: HashMap<XmlElement, PaintId>,
}

impl PaintCache {
    /// Returns an iterator over the cached paints as SVG elements, suitable for inclusion in
    /// `<defs>`.
    fn into_svg(self) -> impl Iterator<Item = XmlElement> {
        let mut paints: Vec<_> = self.paint_to_id.into_iter().collect();
        paints.sort_unstable_by_key(|(_, id)| *id);
        paints
            .into_iter()
            .map(|(grad, id)| grad.with_attribute("id", id))
    }

    /// Returns true if no paints are cached.
    fn is_empty(&self) -> bool {
        self.paint_to_id.is_empty()
    }

    /// Adds a fill attribute to the given path based on the paint, caching gradients if necessary.
    fn add_fill(&mut self, path: &mut XmlElement, paint: &Paint) -> Result<(), DrawSvgError> {
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
            Paint::SweepGradient { .. } => return Err(DrawSvgError::SweepGradientNotSupported),
        };
        Ok(())
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

/// Unique identifier for a paint (solid or gradient).
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PaintId(usize);

impl std::fmt::Display for PaintId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "p{}", self.0)
    }
}

/// Unique identifier for a clip path.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ClipId(usize);

impl std::fmt::Display for ClipId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "c{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        assert_file_eq, assert_matches,
        draw_icon::ViewBoxMode,
        draw_icon::{icon2svg::color_from_u32, DrawIcon, DrawOptions, DrawType},
        error::DrawSvgError,
        iconid::{self, IconIdentifier},
        pathstyle::SvgPathStyle,
        testdata,
    };
    use regex::Regex;
    use skrifa::{prelude::LocationRef, FontRef, GlyphId, MetadataProvider};
    use tiny_skia::Color;

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
            DrawType::Svg,
        )
    }
    fn test_options_bounding_box<'a>(identifier: IconIdentifier) -> DrawOptions<'a> {
        DrawOptions {
            viewbox_mode: ViewBoxMode::UseBoundingBox,
            height: 128.0,
            ..test_options(identifier, LocationRef::default())
        }
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
            &font.draw_icon(&test_options(identifier, &loc)).unwrap(),
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
            &font
                .draw_icon(&DrawOptions {
                    height: 48.0,
                    ..test_options(iconid::MAIL.clone(), &loc)
                })
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
        let font = FontRef::new(testdata::MOSTLY_OFF_CURVE_FONT).unwrap();
        assert_icon_svg_equal(
            testdata::MOSTLY_OFF_CURVE_SVG,
            &font
                .draw_icon(&DrawOptions {
                    viewbox_mode: ViewBoxMode::Auto,
                    ..test_options(IconIdentifier::Codepoint(0x2e), LocationRef::default())
                })
                .unwrap(),
        );
    }

    // This icon was being horribly corrupted initially by compaction
    #[test]
    fn draw_info_icon_unchanged() {
        let font = FontRef::new(testdata::MATERIAL_SYMBOLS_POPULAR).unwrap();
        assert_file_eq!(
            font.draw_icon(&test_options(
                IconIdentifier::Name("info".into()),
                LocationRef::default()
            ),)
                .unwrap(),
            "info_unchanged.svg"
        );
    }

    // This icon was being horribly corrupted initially by compaction
    #[test]
    fn draw_info_icon_compact() {
        let font = FontRef::new(testdata::MATERIAL_SYMBOLS_POPULAR).unwrap();
        assert_file_eq!(
            font.draw_icon(&DrawOptions {
                style: SvgPathStyle::Compact(2),
                ..test_options(IconIdentifier::Name("info".into()), LocationRef::default())
            },)
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
            font.draw_icon(&DrawOptions {
                viewbox_mode: ViewBoxMode::UseHeight,
                ..test_options(iconid::MAIL.clone(), &loc)
            })
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

        let actual_svg = font.draw_icon(&options).unwrap();
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
    fn color_icon_reuses_clip_mask() {
        let font = FontRef::new(testdata::NOTO_EMOJI_FONT).unwrap();
        let svg = font
            .draw_icon(&test_options_bounding_box(IconIdentifier::Codepoint(
                '🥳' as u32,
            )))
            .unwrap();
        assert_file_eq!(svg, "color_icon.svg");
        assert_eq!(svg.matches("<clipPath").count(), 1);
        assert_eq!(svg.matches("url(#c0)").count(), 28);
    }

    #[test]
    fn color_icon_with_duplicate_fill_definitions_reuses_fill_definitions() {
        let font = FontRef::new(testdata::NOTO_EMOJI_FONT).unwrap();
        let svg = font
            .draw_icon(&test_options_bounding_box(
                // Draws 🧜‍♀️ which is glyph id 1760 in the original NotoColorEmoji font.
                IconIdentifier::GlyphId(GlyphId::new(2)),
            ))
            .unwrap();
        assert_file_eq!(svg, "color_icon_reuse_fill.svg");
        assert_eq!(svg.matches("url(#p0)").count(), 2);
    }

    // Sweep gradients are not supported in SVG.
    #[test]
    fn icon_with_sweep_gradient_produces_error() {
        let font = FontRef::new(testdata::COLR_FONT).unwrap();
        assert_matches!(
            font.draw_icon(&test_options_bounding_box(IconIdentifier::Codepoint(
                0xf0200
            ),),),
            Err(DrawSvgError::SweepGradientNotSupported)
        );
    }
}
