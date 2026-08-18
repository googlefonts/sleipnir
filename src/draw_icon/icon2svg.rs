//! Produces svgs of icons in Google-style icon fonts
use std::collections::HashMap;

use super::{draw_glyph, get_pen, DrawOptions, DrawingInstructions, GlyphType};
use crate::{
    error::DrawSvgError,
    pathstyle::SvgPathStyle,
    pens::{ColorDraw, ColorStop, GlyphPainter, Paint},
    xml_element::{HexColor, TruncatedFloat, XmlElement},
};
use kurbo::Affine;
use skrifa::{color::CompositeMode, prelude::Size, FontRef, GlyphId};
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

    let draws = painter.into_draws()?;
    SvgBuilder::default().build(draws, &options.style)
}

/// Builds SVG elements from color drawing instructions.
///
/// Accumulates reusable definitions such as clip paths, gradients, and masks
/// into `<defs>` while constructing the SVG element hierarchy.
#[derive(Default)]
struct SvgBuilder {
    /// Cache of clip paths to be emitted into `<defs>`.
    clips: ClipsCache,
    /// Cache of gradient and paint definitions to be emitted into `<defs>`.
    fills: PaintCache,
    /// Cache of mask definitions used for compositing and blending operations.
    masks: MasksCache,
}

impl SvgBuilder {
    /// Converts a sequence of [`ColorDraw`] instructions into a root SVG element.
    ///
    /// If any clips, masks, or paint definitions were generated during rendering,
    /// a `<defs>` element containing them is appended to the output.
    fn build(
        mut self,
        draws: Vec<ColorDraw>,
        style: &SvgPathStyle,
    ) -> Result<XmlElement, DrawSvgError> {
        let mut group = self.draws_to_svg_elements(&draws, style)?;

        if !self.fills.is_empty() || !self.clips.is_empty() || !self.masks.is_empty() {
            group.push(
                XmlElement::new("defs")
                    .with_children(self.clips.into_svg())
                    .with_children(self.masks.into_svg())
                    .with_children(self.fills.into_svg()),
            );
        }

        let xml = match group.len() {
            1 => group.into_iter().next().unwrap(),
            _ => XmlElement::new("g").with_children(group),
        };
        Ok(xml)
    }

    /// Converts a slice of [`ColorDraw`] instructions into a list of SVG elements.
    ///
    /// Handles paths, fills, nested clipping hierarchies, translations, and composite layers.
    fn draws_to_svg_elements(
        &mut self,
        draws: &[ColorDraw],
        style: &SvgPathStyle,
    ) -> Result<Vec<XmlElement>, DrawSvgError> {
        let mut elements = Vec::new();
        for draw in draws {
            match draw {
                ColorDraw::Fill(fill) => {
                    let [clips @ .., shape] = fill.clip_paths.as_slice() else {
                        continue;
                    };
                    let mut path =
                        XmlElement::new("path").with_attribute("d", style.write_svg_path(shape));

                    self.fills.add_fill(&mut path, &fill.paint)?;

                    let mut clip_parent_id = None;
                    for clip in clips {
                        let id = self
                            .clips
                            .get_id(clip_parent_id, style.write_svg_path(clip).to_string());
                        clip_parent_id = Some(id);
                    }
                    if let Some(id) = clip_parent_id {
                        path.add_attribute("clip-path", format!("url(#{})", id));
                    }

                    if fill.offset_x != 0.0 || fill.offset_y != 0.0 {
                        path.add_attribute(
                            "transform",
                            format!("translate({} {})", fill.offset_x, fill.offset_y),
                        );
                    }

                    elements.push(path);
                }
                ColorDraw::Layer { mode, draws } => {
                    self.apply_layer(*mode, draws, &mut elements, style)?;
                }
            }
        }
        Ok(elements)
    }

    /// Applies layer compositing or blending modes to child draw operations against current elements.
    ///
    /// Implements Porter-Duff compositing modes (such as `SrcIn`, `DestOut`, `Xor`) using SVG
    /// masks, and blend modes (such as `Multiply`, `Screen`, `Overlay`) using CSS blend styles.
    fn apply_layer(
        &mut self,
        mode: CompositeMode,
        draws: &[ColorDraw],
        elements: &mut Vec<XmlElement>,
        style: &SvgPathStyle,
    ) -> Result<(), DrawSvgError> {
        match mode {
            CompositeMode::Clear => {
                elements.clear();
            }
            CompositeMode::Src => {
                let child_elements = self.draws_to_svg_elements(draws, style)?;
                *elements = child_elements;
            }
            CompositeMode::Dest => {
                // Discard source (draws), keep backdrop (elements).
            }
            CompositeMode::DestOver => {
                let mut child_elements = self.draws_to_svg_elements(draws, style)?;
                child_elements.extend(std::mem::take(elements));
                *elements = child_elements;
            }
            CompositeMode::SrcIn | CompositeMode::SrcOut => {
                let mask_elements = std::mem::take(elements);
                let child_elements = self.draws_to_svg_elements(draws, style)?;
                let inverted = mode == CompositeMode::SrcOut;
                elements.push(
                    self.masks
                        .masked_group(child_elements, mask_elements, inverted),
                );
            }
            CompositeMode::DestIn | CompositeMode::DestOut => {
                let child_elements = self.draws_to_svg_elements(draws, style)?;
                let backdrop_elements = std::mem::take(elements);
                let inverted = mode == CompositeMode::DestOut;
                elements.push(
                    self.masks
                        .masked_group(backdrop_elements, child_elements, inverted),
                );
            }
            CompositeMode::SrcAtop => {
                let backdrop_elements = std::mem::take(elements);
                let child_elements = self.draws_to_svg_elements(draws, style)?;
                let source_group =
                    self.masks
                        .masked_group(child_elements, backdrop_elements.clone(), false);
                *elements = backdrop_elements;
                elements.push(source_group);
            }
            CompositeMode::DestAtop => {
                let backdrop_elements = std::mem::take(elements);
                let child_elements = self.draws_to_svg_elements(draws, style)?;
                let backdrop_group =
                    self.masks
                        .masked_group(backdrop_elements, child_elements.clone(), false);
                *elements = child_elements;
                elements.push(backdrop_group);
            }
            CompositeMode::Xor => {
                let backdrop_elements = std::mem::take(elements);
                let child_elements = self.draws_to_svg_elements(draws, style)?;
                let source_group = self.masks.masked_group(
                    child_elements.clone(),
                    backdrop_elements.clone(),
                    true,
                );
                let backdrop_group =
                    self.masks
                        .masked_group(backdrop_elements, child_elements, true);
                *elements = vec![backdrop_group, source_group];
            }
            CompositeMode::SrcOver => {
                let child_elements = self.draws_to_svg_elements(draws, style)?;
                let group = XmlElement::new("g")
                    .with_children(child_elements)
                    .with_attribute("style", "isolation: isolate");
                elements.push(group);
            }
            _ => {
                let child_elements = self.draws_to_svg_elements(draws, style)?;
                let blend_mode = match mode {
                    CompositeMode::Multiply => Ok("multiply"),
                    CompositeMode::Screen => Ok("screen"),
                    CompositeMode::Overlay => Ok("overlay"),
                    CompositeMode::Darken => Ok("darken"),
                    CompositeMode::Lighten => Ok("lighten"),
                    CompositeMode::ColorDodge => Ok("color-dodge"),
                    CompositeMode::ColorBurn => Ok("color-burn"),
                    CompositeMode::HardLight => Ok("hard-light"),
                    CompositeMode::SoftLight => Ok("soft-light"),
                    CompositeMode::Difference => Ok("difference"),
                    CompositeMode::Exclusion => Ok("exclusion"),
                    CompositeMode::HslHue => Ok("hue"),
                    CompositeMode::HslSaturation => Ok("saturation"),
                    CompositeMode::HslColor => Ok("color"),
                    CompositeMode::HslLuminosity => Ok("luminosity"),
                    CompositeMode::Plus => Ok("plus-lighter"),
                    unsupported => Err(DrawSvgError::CompositeModeNotSupported(unsupported)),
                }?;
                let group = XmlElement::new("g")
                    .with_children(child_elements)
                    .with_attribute("style", format!("mix-blend-mode: {blend_mode}"));
                elements.push(group);
            }
        }
        Ok(())
    }
}

/// Unique identifier for a mask.
///
/// Wraps a sequential numeric index corresponding to the position in [`MasksCache::masks`],
/// which is formatted as `"m{index}"` (e.g., `"m0"`, `"m1"`) when referenced by SVG `mask="url(#m0)"`.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct MaskId(usize);

impl std::fmt::Display for MaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "m{}", self.0)
    }
}

/// Manages SVG masks in `<defs>`.
#[derive(Default)]
struct MasksCache {
    /// List of `<mask id="...">` SVG elements to be included in `<defs>`.
    masks: Vec<XmlElement>,
}

impl MasksCache {
    /// Wraps elements in a `<g>` group masked by the given `mask` elements.
    ///
    /// Registers the mask with this cache (as an inverted mask if `inverted` is `true`)
    /// and attaches the resulting `mask="url(#m...)"` attribute to the group.
    fn masked_group(
        &mut self,
        content: Vec<XmlElement>,
        mask: Vec<XmlElement>,
        inverted: bool,
    ) -> XmlElement {
        let mask_id = if inverted {
            self.add_inverted_mask(mask)
        } else {
            self.add_mask(mask)
        };
        XmlElement::new("g")
            .with_children(content)
            .with_attribute("mask", format!("url(#{mask_id})"))
    }

    /// Adds a mask containing the given SVG elements and returns its [`MaskId`].
    fn add_mask(&mut self, mask_content: Vec<XmlElement>) -> MaskId {
        let mask_id = MaskId(self.masks.len());
        let children: Vec<XmlElement> = mask_content
            .into_iter()
            .map(|mut el| {
                Self::set_fill_color_recursive(&mut el, "#ffffff");
                el
            })
            .collect();
        self.masks.push(
            XmlElement::new("mask")
                .with_attribute("id", mask_id)
                .with_children(children),
        );
        mask_id
    }

    /// Adds an inverted mask where the given elements block visibility (black) on a white canvas.
    fn add_inverted_mask(&mut self, mask_content: Vec<XmlElement>) -> MaskId {
        let mask_id = MaskId(self.masks.len());
        let mut children = vec![XmlElement::new("rect")
            .with_attribute("x", "-100%")
            .with_attribute("y", "-100%")
            .with_attribute("width", "300%")
            .with_attribute("height", "300%")
            .with_attribute("fill", "#ffffff")];
        children.extend(mask_content.into_iter().map(|mut el| {
            Self::set_fill_color_recursive(&mut el, "#000000");
            el
        }));
        self.masks.push(
            XmlElement::new("mask")
                .with_attribute("id", mask_id)
                .with_children(children),
        );
        mask_id
    }

    fn set_fill_color_recursive(el: &mut XmlElement, target_color: &'static str) {
        let mut alpha_val = None;
        for (k, v) in el.attributes() {
            if k == "fill" && v.starts_with('#') && v.len() == 9 {
                if let Ok(alpha) = u8::from_str_radix(&v[7..9], 16) {
                    alpha_val = Some(alpha as f64 / 255.0);
                }
            }
        }
        el.set_attribute("fill", target_color);
        if let Some(alpha) = alpha_val {
            if (alpha - 1.0).abs() > 0.001 {
                el.set_attribute("fill-opacity", crate::xml_element::TruncatedFloat(alpha));
            }
        }
        for child in el.children_mut() {
            Self::set_fill_color_recursive(child, target_color);
        }
    }

    /// Returns an iterator over the mask elements, suitable for inclusion in `<defs>`.
    fn into_svg(self) -> impl Iterator<Item = XmlElement> {
        self.masks.into_iter()
    }

    /// Returns true if no masks have been added.
    fn is_empty(&self) -> bool {
        self.masks.is_empty()
    }
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

    #[test]
    fn composite_modes() {
        let font = FontRef::new(testdata::COLR_FONT).unwrap();
        let modes = [
            (0xf0a00_u32, "clear"),
            (0xf0a01, "src"),
            (0xf0a02, "dest"),
            (0xf0a03, "src_over"),
            (0xf0a04, "dest_over"),
            (0xf0a05, "src_in"),
            (0xf0a06, "dest_in"),
            (0xf0a07, "src_out"),
            (0xf0a08, "dest_out"),
            (0xf0a09, "src_atop"),
            (0xf0a0a, "dest_atop"),
            (0xf0a0b, "xor"),
            (0xf0a0c, "plus"),
            (0xf0a0d, "screen"),
            (0xf0a0e, "overlay"),
            (0xf0a0f, "darken"),
            (0xf0a10, "lighten"),
            (0xf0a11, "color_dodge"),
            (0xf0a12, "color_burn"),
            (0xf0a13, "hard_light"),
            (0xf0a14, "soft_light"),
            (0xf0a15, "difference"),
            (0xf0a16, "exclusion"),
            (0xf0a17, "multiply"),
            (0xf0a18, "hsl_hue"),
            (0xf0a19, "hsl_saturation"),
            (0xf0a1a, "hsl_color"),
            (0xf0a1b, "hsl_luminosity"),
        ];

        for (cp, name) in modes {
            let options = DrawOptions {
                viewbox_mode: ViewBoxMode::UseBoundingBox,
                ..DrawOptions::new(
                    IconIdentifier::Codepoint(cp),
                    64.0,
                    LocationRef::default(),
                    SvgPathStyle::Compact(2),
                    DrawType::Svg,
                )
            };

            let svg = font
                .draw_icon(&options)
                .unwrap_or_else(|e| panic!("Failed to draw icon for composite mode {name}: {e:?}"));
            assert_file_eq!(svg, &format!("composite_modes/{name}.svg"));
        }
    }
}
