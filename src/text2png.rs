//! renders text into png, forked from <https://github.com/rsheeter/embed1/blob/main/make_test_images/src/main.rs>
use kurbo::{BezPath, PathEl, Rect, Shape, Vec2};
use skrifa::{
    color::{ColorPainter, PaintError},
    prelude::{LocationRef, Size},
    raw::{FontRef, ReadError},
    GlyphId, MetadataProvider,
};
use thiserror::Error;
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Transform};

use crate::{measure::shape, pens::ColorPainterImpl};

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
    #[error("{0}")]
    PaintError(PaintError),
    #[error("glyph {0} not found")]
    GlyphNotFound(GlyphId),
    #[error("Unsupported font feature: {0}")]
    UnsupportedFontFeature(&'static str),
}

// TODO: From<PaintError> can be autoderived with `#[from]` once
// `PaintError` implements `Error`.
impl From<PaintError> for TextToPngError {
    fn from(err: PaintError) -> TextToPngError {
        TextToPngError::PaintError(err)
    }
}

fn compute_bounds(fills: &[crate::pens::ColorFill]) -> Rect {
    let mut bounds: Option<Rect> = None;
    for fill in fills.iter() {
        let b = fill.path.bounding_box() + Vec2::new(fill.offset_x, fill.offset_y);
        bounds = Some(match bounds {
            None => b,
            Some(bbox) => bbox.union(b),
        })
    }
    bounds.unwrap_or_default()
}

fn to_pixmap(
    fills: &[crate::pens::ColorFill],
    background: Color,
    height: f64,
) -> Result<Pixmap, TextToPngError> {
    let bounds = compute_bounds(fills);
    let width = bounds.width();

    let mut pixmap = Pixmap::new(width.ceil() as u32, height.ceil() as u32)
        .ok_or(TextToPngError::TextTooSmall)?;
    pixmap.fill(background);
    let x_offset = -bounds.min_x();
    let y_offset_for_centering = (height - bounds.height()) / 2.0;
    let y_offset = y_offset_for_centering - bounds.min_y();
    for fill in fills {
        pixmap.fill_path(
            &kurbo_path_to_skia(&fill.path)?,
            &fill.paint,
            FillRule::Winding,
            Transform::from_translate(
                (fill.offset_x + x_offset) as f32,
                (fill.offset_y + y_offset) as f32,
            ),
            None,
        );
    }
    Ok(pixmap)
}

pub fn with_margin(rect: Rect, multiplier: f64) -> Rect {
    let margin = rect.width().min(rect.height()) * multiplier;
    rect.inflate(margin, margin)
}

fn kurbo_path_to_skia(path: &BezPath) -> Result<tiny_skia::Path, TextToPngError> {
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
    if text.split_whitespace().count() == 0 {
        return Err(TextToPngError::NoText);
    }

    let size = Size::new(font_size);
    let location = LocationRef::default();
    let metrics = font.metrics(size, location);
    let line_height = line_spacing as f64 * font_size as f64;
    let scale = 1.0 / metrics.units_per_em as f64 * font_size as f64;
    let mut painter = ColorPainterImpl::new(&font, size, foreground, scale as f32);
    for (line_num, text) in text.lines().enumerate() {
        let glyphs = shape(text, &font);
        painter.x = 0.0;
        painter.y = line_num as f64 * line_height;
        for (glyph_info, pos) in glyphs.glyph_infos().iter().zip(glyphs.glyph_positions()) {
            let glyph_id = glyph_info.glyph_id.into();
            match color_glyphs.get(glyph_id) {
                // Color glyphs have their own rich set of instructions that interact with the
                // painter.
                Some(color_glyph) => color_glyph.paint(location, &mut painter)?,
                None => {
                    let paint = Paint {
                        shader: tiny_skia::Shader::SolidColor(foreground),
                        ..Paint::default()
                    };
                    painter.push_clip_glyph(glyph_id);
                    painter.add_fill(paint);
                    // The painter requires popping the glyph. If not, then the current glyph should
                    // be used as a mask for the next glyph.
                    painter.pop_clip();
                }
            };
            painter.x += pos.x_advance as f64 * scale;
        }
    }
    let expected_height = (line_spacing * font_size * text.lines().count() as f32) as f64;
    let pixmap = to_pixmap(&painter.fills()?, background, expected_height)?;
    let bytes = pixmap.encode_png()?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use tiny_skia::Color;

    use crate::{
        assert_file_eq, assert_matches, testdata, text2png::text2png, text2png::TextToPngError,
    };

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
    fn unmapped_character_produces_error() {
        assert_matches!(
            text2png(
                // "c" is not included in our subsetted NABLA_FONT used for testing.
                "c",
                64.0,
                1.0,
                testdata::NABLA_FONT,
                Color::BLACK,
                Color::WHITE,
            ),
            // TODO: Produce a better error.
            Err(TextToPngError::PathBuildError)
        );
    }

    #[test]
    fn whitespace_only_produces_error() {
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
            Err(TextToPngError::NoText)
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
            Err(TextToPngError::NoText)
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
        assert_matches!(result1, Err(TextToPngError::TextTooSmall));

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
