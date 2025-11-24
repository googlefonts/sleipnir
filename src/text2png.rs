//! renders text into png, forked from <https://github.com/rsheeter/embed1/blob/main/make_test_images/src/main.rs>
use kurbo::{Affine, BezPath, PathEl, Rect, Shape, Vec2};
use skrifa::{
    outline::DrawSettings,
    prelude::{LocationRef, Size},
    raw::{tables::colr::Colr, FontRef, ReadError, TableProvider},
    MetadataProvider,
};
use thiserror::Error;
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Transform};

use crate::{measure::shape, pens::SvgPathPen};

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
}

// TODO: add Location (aka VF settings) or DrawOptions without identifier
fn text_to_bez_path(font: &FontRef, text: &str, font_size: f32, line_spacing: f32) -> BezPath {
    let outlines = font.outline_glyphs();
    let mut pen = BezPath::new();

    let size = Size::new(font_size);
    let location = LocationRef::default();
    let metrics = font.metrics(size, location);
    let line_height = line_spacing * font_size;
    let scale = 1.0 / metrics.units_per_em as f32 * font_size;

    for (line_num, text) in text.lines().enumerate() {
        let mut line_pen = BezPath::default();
        let mut x_offset = 0.0;

        let glyphs = shape(text, font);

        for (glyph_info, pos) in glyphs.glyph_infos().iter().zip(glyphs.glyph_positions()) {
            let glyph = outlines
                .get(glyph_info.glyph_id.into())
                .expect("Glyphs to exist!");

            let mut glyph_pen = SvgPathPen::new();
            glyph
                .draw(DrawSettings::unhinted(size, location), &mut glyph_pen)
                .expect("To draw!");

            let mut glyph_path = glyph_pen.into_inner();
            glyph_path.apply_affine(Affine::translate(Vec2 {
                x: x_offset as f64,
                y: 0.0,
            }));
            line_pen.extend(glyph_path);

            x_offset += pos.x_advance as f32 * scale;
        }

        let y_offset: f32 = line_height * line_num as f32;
        line_pen.apply_affine(Affine::translate(Vec2 {
            x: 0.0,
            y: y_offset as f64,
        }));
        pen.extend(line_pen);
    }
    pen
}

pub fn with_margin(rect: Rect, multiplier: f64) -> Rect {
    let margin = rect.width().min(rect.height()) * multiplier;
    rect.inflate(margin, margin)
}

fn colored_text_to_png(
    text: &str,
    font_size: f32,
    line_spacing: f32,
    font: FontRef,
    colr: Colr,
    background: Color,
) -> Result<Vec<u8>, TextToPngError> {
    todo!();
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
    if let Ok(colr) = font.colr() {
        return colored_text_to_png(text, font_size, line_spacing, font, colr, background);
    }

    let expected_height = (line_spacing * font_size * text.lines().count() as f32) as f64;

    let mut bez_path = text_to_bez_path(&font, text, font_size, line_spacing);
    let old_bbox = bez_path.bounding_box();
    bez_path.apply_affine(Affine::translate(Vec2 {
        x: -old_bbox.min_x(),
        y: -old_bbox.min_y(),
    }));
    let bbox = bez_path.bounding_box();

    if bbox.area() == 0.0 {
        return Err(TextToPngError::NoText);
    }

    let y_offset = (expected_height - bbox.height()) / 2.0;

    let mut pixmap = Pixmap::new(bbox.width().ceil() as u32, expected_height as u32)
        .ok_or(TextToPngError::TextTooSmall)?;

    // https://github.com/linebender/tiny-skia/blob/main/examples/fill.rs basically
    pixmap.fill(background);

    let skia_path = {
        let mut pb = PathBuilder::new();
        for el in bez_path {
            match el {
                PathEl::MoveTo(p) => pb.move_to(p.x as f32, p.y as f32),
                PathEl::LineTo(p) => pb.line_to(p.x as f32, p.y as f32),
                PathEl::QuadTo(c0, p) => {
                    pb.quad_to(c0.x as f32, c0.y as f32, p.x as f32, p.y as f32)
                }
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
        pb.finish().ok_or(TextToPngError::PathBuildError)?
    };
    pixmap.fill_path(
        &skia_path,
        &paint_with_foreground(foreground),
        FillRule::Winding,
        Transform::from_translate(0.0, y_offset as f32),
        None,
    );
    let png_bytes = pixmap.encode_png()?;
    Ok(png_bytes)
}

fn paint_with_foreground(foreground: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(foreground);
    p
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

        assert_eq!(
            actual_bytes, expected_bytes,
            "Bytes ({actual_path:?}) did not match bytes from {expected_path:?}"
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
