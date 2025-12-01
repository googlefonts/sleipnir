//! renders text into png, forked from <https://github.com/rsheeter/embed1/blob/main/make_test_images/src/main.rs>
use kurbo::{Affine, BezPath, PathEl, Point, Rect, Shape};
use skrifa::{
    color::ColorPainter,
    outline::DrawSettings,
    prelude::{LocationRef, Size},
    raw::{tables::cpal::ColorRecord, FontRef, ReadError, TableProvider},
    MetadataProvider, OutlineGlyphCollection,
};
use thiserror::Error;
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Transform};

use crate::{measure::shape, pens::SvgPathPen};

fn spread_mode(extend: skrifa::color::Extend) -> tiny_skia::SpreadMode {
    match extend {
        skrifa::color::Extend::Pad => tiny_skia::SpreadMode::Pad,
        skrifa::color::Extend::Repeat => tiny_skia::SpreadMode::Repeat,
        skrifa::color::Extend::Reflect => tiny_skia::SpreadMode::Reflect,
        // `Extend` requires non-exhaustive matching. If any new
        // variants are discovered, they should be added.
        _ => tiny_skia::SpreadMode::Pad,
    }
}

#[derive(Error, Debug)]
pub enum TextToPngError {
    #[error("error reading font bytes: {0}")]
    ReadError(#[from] ReadError),
    #[error("error encoding bitmap to png: {0}")]
    PngEncodingError(#[from] png::EncodingError),
    #[error("there was no text to render")]
    NoText,
    #[error("the combination of text and font size was too small to produce anything")]
    TextTooSmall,
    #[error("Failed to build render path")]
    PathBuildError,
    #[error("Unssuported font feature: {0}")]
    UnsupportedFontFeature(&'static str),
}

pub fn with_margin(rect: Rect, multiplier: f64) -> Rect {
    let margin = rect.width().min(rect.height()) * multiplier;
    rect.inflate(margin, margin)
}

pub struct ColorPainterImpl<'a> {
    x: f64,
    y: f64,
    size: Size,
    foreground: Color,
    scale: f32,
    outlines: OutlineGlyphCollection<'a>,
    colors: &'a [ColorRecord],
    path: BezPath,
    err: Option<TextToPngError>,
    fills: Vec<ColorFill<'a>>,
}

#[derive(Debug)]
pub struct ColorFill<'a> {
    paint: Paint<'a>,
    path: BezPath,
    offset_x: f64,
    offset_y: f64,
}

impl<'a> ColorPainterImpl<'a> {
    fn new(font: &FontRef<'a>, size: Size, foreground: Color, scale: f32) -> Self {
        let outlines = font.outline_glyphs();
        let cpal = font.cpal();
        let colors = match cpal.map(|c| c.color_records_array()) {
            Ok(Some(Ok(c))) => c,
            _ => &[],
        };
        ColorPainterImpl {
            x: 0.0,
            y: 0.0,
            size,
            foreground,
            scale,
            outlines,
            colors,
            path: BezPath::default(),
            err: None,
            fills: Vec::new(),
        }
    }

    fn color(&mut self, palette_idx: u16, alpha: f32) -> tiny_skia::Color {
        if palette_idx == 0xFFFF {
            let mut color = self.foreground;
            color.set_alpha(alpha);
            return color;
        }
        let Some(color) = self.colors.get(palette_idx as usize) else {
            if self.err.is_none() {
                self.err = Some(TextToPngError::UnsupportedFontFeature(
                    "Color palette index out of bounds.",
                ));
            }
            return self.foreground;
        };
        let max = u8::MAX as f32;
        Color::from_rgba8(
            color.red,
            color.green,
            color.blue,
            (alpha * max).clamp(0.0, max) as u8,
        )
    }

    fn add_fill(&mut self, paint: Paint<'a>) {
        let path = self.path.clone();
        let fill = ColorFill {
            paint,
            path,
            offset_x: self.x,
            offset_y: self.y,
        };
        self.fills.push(fill);
    }

    fn to_pixmap(&self, background: Color, height: f64) -> Result<Pixmap, TextToPngError> {
        let all_paths: Vec<PathEl> = self
            .fills
            .iter()
            .map(|f| {
                f.path
                    .iter()
                    .map(|el: PathEl| Affine::translate((f.offset_x as f64, f.offset_y)) * el)
            })
            .flatten()
            .collect();
        let all_paths = BezPath::from_vec(all_paths);
        let bounds = all_paths.bounding_box();
        let width = bounds.width();
        let y_offset = (height - bounds.height()) / 2.0;

        let mut pixmap = Pixmap::new(width.ceil() as u32, height.ceil() as u32)
            .ok_or(TextToPngError::TextTooSmall)?;
        pixmap.fill(background);
        for fill in &self.fills {
            let path = kurbo_to_skia(&fill.path)?;
            let transform = Transform::from_translate(
                (fill.offset_x - bounds.min_x()) as f32,
                (fill.offset_y - bounds.min_y() + y_offset) as f32,
            );
            pixmap.fill_path(&path, &fill.paint, FillRule::Winding, transform, None);
        }
        Ok(pixmap)
    }

    fn is_good(&self) -> bool {
        self.err.is_none()
    }
}

impl<'a> ColorPainter for ColorPainterImpl<'a> {
    fn push_transform(&mut self, _: skrifa::color::Transform) {
        if !self.is_good() {
            return;
        }
        self.err = Some(TextToPngError::UnsupportedFontFeature(
            "Transforms are not supported.",
        ));
    }

    fn pop_transform(&mut self) {
        if !self.is_good() {
            return;
        }
        self.err = Some(TextToPngError::UnsupportedFontFeature(
            "Transforms are not supported.",
        ));
    }

    fn push_clip_glyph(&mut self, glyph_id: skrifa::GlyphId) {
        if !self.is_good() {
            return;
        }
        if !self.path.is_empty() {
            self.err = Some(TextToPngError::UnsupportedFontFeature(
                "Invalid state: Path not empty when pushing clip glyph.",
            ));
            return;
        }
        let Some(glyph) = self.outlines.get(glyph_id) else {
            eprintln!("Did not find an outline");
            return;
        };
        let location = LocationRef::default();
        let mut path_pen = SvgPathPen::new();
        glyph
            .draw(DrawSettings::unhinted(self.size, location), &mut path_pen)
            .unwrap();
        self.path = path_pen.into_inner();
    }

    fn push_clip_box(&mut self, clip_box: skrifa::metrics::BoundingBox) {
        if !self.is_good() {
            return;
        }
        if !self.path.is_empty() {
            self.err = Some(TextToPngError::PathBuildError);
            return;
        }
        self.path.extend(
            [
                PathEl::MoveTo(Point::new(clip_box.x_min as f64, clip_box.y_min as f64)),
                PathEl::LineTo(Point::new(clip_box.x_max as f64, clip_box.y_min as f64)),
                PathEl::LineTo(Point::new(clip_box.x_max as f64, clip_box.y_max as f64)),
                PathEl::LineTo(Point::new(clip_box.x_min as f64, clip_box.y_max as f64)),
                PathEl::ClosePath,
            ]
            .into_iter(),
        );
    }

    fn pop_clip(&mut self) {
        if !self.is_good() {
            return;
        }
        self.path.truncate(0);
    }

    fn fill(&mut self, brush: skrifa::color::Brush<'_>) {
        if !self.is_good() {
            return;
        }
        let paint = match brush {
            skrifa::color::Brush::Solid {
                palette_index,
                alpha,
            } => Paint {
                shader: tiny_skia::Shader::SolidColor(self.color(palette_index, alpha)),
                ..Paint::default()
            },
            skrifa::color::Brush::LinearGradient {
                p0,
                p1,
                color_stops,
                extend,
            } => {
                let color_stops = color_stops
                    .iter()
                    .map(|stop| {
                        let color = self.color(stop.palette_index, stop.alpha);
                        tiny_skia::GradientStop::new(stop.offset, color)
                    })
                    .collect();
                let gradient = tiny_skia::LinearGradient::new(
                    tiny_skia::Point::from_xy(p0.x, -p0.y),
                    tiny_skia::Point::from_xy(p1.x, -p1.y),
                    color_stops,
                    spread_mode(extend),
                    tiny_skia::Transform::from_scale(self.scale, self.scale),
                )
                .unwrap();
                Paint {
                    shader: gradient,
                    ..Paint::default()
                }
            }
            skrifa::color::Brush::RadialGradient { .. } => {
                self.err = Some(TextToPngError::UnsupportedFontFeature(
                    "Radial gradients are not supported.",
                ));
                return;
            }
            skrifa::color::Brush::SweepGradient { .. } => {
                self.err = Some(TextToPngError::UnsupportedFontFeature(
                    "Sweep gradients are not supported.",
                ));
                return;
            }
        };
        self.add_fill(paint);
    }

    fn push_layer(&mut self, _: skrifa::color::CompositeMode) {
        if !self.is_good() {
            return;
        }
        self.err = Some(TextToPngError::UnsupportedFontFeature(
            "Layers are not supported.",
        ));
    }
}

fn kurbo_to_skia(path: &BezPath) -> Result<tiny_skia::Path, TextToPngError> {
    let mut pb = PathBuilder::new();
    for el in path {
        match el {
            PathEl::MoveTo(p) => pb.move_to(p.x as f32, p.y as f32),
            PathEl::LineTo(p) => pb.line_to(p.x as f32, p.y as f32),
            PathEl::QuadTo(c0, p) => pb.quad_to(c0.x as f32, c0.y as f32, p.x as f32, p.y as f32),
            PathEl::CurveTo(c0, c1, p) => pb.cubic_to(
                c0.x as f32,
                c0.y as f32,
                c1.x as f32,
                c1.y as f32,
                p.x as f32,
                p.y as f32,
            ),
            PathEl::ClosePath => pb.close(),
        }
    }
    pb.finish().ok_or(TextToPngError::PathBuildError)
}

pub fn text2png(
    text: &str,
    font_size: f32,
    line_spacing: f32,
    font_bytes: &[u8],
    foreground: Color,
    background: Color,
) -> Result<Vec<u8>, TextToPngError> {
    let font = FontRef::new(font_bytes)?;
    let color_glyphs = font.color_glyphs();

    let size = Size::new(font_size);
    let location = LocationRef::default();
    let metrics = font.metrics(size, location);
    let line_height = line_spacing as f64 * font_size as f64;
    let scale = 1.0 / metrics.units_per_em as f64 * font_size as f64;
    let mut svg_builder = ColorPainterImpl::new(&font, size, foreground, scale as f32);

    for (line_num, text) in text.lines().enumerate() {
        let glyphs = shape(text, &font);
        svg_builder.x = 0.0;
        svg_builder.y = line_num as f64 * line_height;
        for (glyph_info, pos) in glyphs.glyph_infos().iter().zip(glyphs.glyph_positions()) {
            let glyph_id = glyph_info.glyph_id.into();
            match color_glyphs.get(glyph_id) {
                None => {
                    let paint = Paint {
                        shader: tiny_skia::Shader::SolidColor(foreground),
                        ..Paint::default()
                    };
                    svg_builder.push_clip_glyph(glyph_id);
                    svg_builder.add_fill(paint);
                    svg_builder.pop_clip();
                }
                Some(color_glyph) => color_glyph.paint(location, &mut svg_builder).unwrap(),
            };
            svg_builder.x += pos.x_advance as f64 * scale;
        }
    }
    let expected_height = (line_spacing * font_size * text.lines().count() as f32) as f64;
    let pixmap = svg_builder.to_pixmap(background, expected_height)?;
    let bytes = pixmap.encode_png()?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tiny_skia::Color;

    use crate::{
        assert_file_eq, assert_matches, testdata, text2png::text2png, text2png::TextToPngError,
    };

    #[track_caller]
    fn assert_file_eq_impl(actual_bytes: &[u8], file: &str) {
        let expected_path = PathBuf::from_iter(["resources/testdata", file]);
        let expected_bytes = std::fs::read(&expected_path)
            .inspect_err(|err| eprintln!("Failed to read {expected_path:?}: {err}"))
            .unwrap_or_default();

        let actual_dir = "target/testdata";
        if let Err(err) = std::fs::create_dir_all(actual_dir) {
            eprintln!("Failed to create target/testdata directory: {err}");
        }
        let actual_path = PathBuf::from_iter([actual_dir, file]);
        if let Err(err) = std::fs::write(&actual_path, actual_bytes) {
            eprintln!("Failed to write actual bytes to {actual_path:?}: {err}");
        }

        assert!(
            actual_bytes == expected_bytes,
            "Bytes (stored in{actual_path:?}) did not match expected bytes from {expected_path:?}"
        );
    }

    macro_rules! assert_file_eq {
        ($actual:expr, $expected_file:expr) => {
            assert_file_eq_impl(&$actual, $expected_file);
        };
    }

    #[test]
    fn ligature() {
        let png_bytes = text2png(
            "fitto",
            24.0,
            1.0,
            testdata::CAVEAT_FONT,
            Color::from_rgba8(255, 255, 255, 255),
            Color::from_rgba8(20, 20, 20, 255),
        )
        .expect("To draw PNG");

        assert_file_eq!(png_bytes, "render_ligature.png");
    }

    #[test]
    fn two_lines() {
        let png_bytes = text2png(
            "hello\nworld",
            24.0,
            1.0,
            testdata::CAVEAT_FONT,
            Color::from_rgba8(255, 255, 255, 255),
            Color::from_rgba8(20, 20, 20, 255),
        )
        .expect("To draw PNG");

        assert_file_eq!(png_bytes, "render_two_lines.png");
    }

    #[test]
    fn colored_font() {
        let png_bytes = text2png(
            "abab\nABAB",
            64.0,
            1.0,
            testdata::NABLA_FONT,
            Color::BLACK,
            Color::WHITE,
        )
        .unwrap();
        assert_file_eq!(png_bytes, "colored_font.png");
    }

    #[test]
    fn empty_string_produces_error() {
        let result = text2png(
            "",
            24.0,
            1.0,
            testdata::CAVEAT_FONT,
            Color::WHITE,
            Color::BLACK,
        );
        assert_matches!(result, Err(TextToPngError::NoText));
    }

    #[test]
    fn whitespace_may_produce_error() {
        let result = text2png(
            "\n \n",
            24.0,
            1.0,
            testdata::CAVEAT_FONT,
            Color::WHITE,
            Color::BLACK,
        );
        assert_matches!(result, Err(TextToPngError::NoText));

        assert_matches!(
            text2png(
                "\r",
                24.0,
                1.0,
                testdata::CAVEAT_FONT,
                Color::WHITE,
                Color::BLACK,
            ),
            Ok(_)
        );
        assert_matches!(
            text2png(
                "\t",
                24.0,
                1.0,
                testdata::CAVEAT_FONT,
                Color::WHITE,
                Color::BLACK,
            ),
            Ok(_)
        );
    }

    #[test]
    fn bad_font_data_produces_error() {
        let bad_font_data = &[];
        let result = text2png(
            "hello world",
            24.0,
            1.0,
            bad_font_data,
            Color::WHITE,
            Color::BLACK,
        );
        assert_matches!(result, Err(TextToPngError::ReadError(_)));
    }

    #[test]
    fn zero_size_font_produces_error() {
        let result1 = text2png(
            "hello",
            0.0,
            1.0,
            testdata::CAVEAT_FONT,
            Color::WHITE,
            Color::BLACK,
        );
        assert_matches!(result1, Err(TextToPngError::NoText));

        let result2 = text2png(
            "hello",
            12.0,
            0.0,
            testdata::CAVEAT_FONT,
            Color::WHITE,
            Color::BLACK,
        );
        assert_matches!(result2, Err(TextToPngError::TextTooSmall));
    }
}
