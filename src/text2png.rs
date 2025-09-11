//! renders text into png, forked from <https://github.com/rsheeter/embed1/blob/main/make_test_images/src/main.rs>
use harfrust::FontRef;
use kurbo::{Affine, BezPath, PathEl, Rect, Shape, Vec2};
use png::EncodingError;
use skrifa::{
    outline::DrawSettings,
    prelude::{LocationRef, Size},
    FontRef as SkrifaFontRef, MetadataProvider,
};
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Transform};

use crate::{measure::shape, pens::SvgPathPen};

// TODO: add Location (aka VF settings) or DrawOptions without identifier
fn draw_text(
    harf_font: &FontRef,
    skrifa_font: &SkrifaFontRef,
    text: &str,
    font_size: f32,
    line_spacing: f32,
) -> BezPath {
    let outlines = skrifa_font.outline_glyphs();
    let mut pen = BezPath::new();

    let size = Size::new(font_size);
    let location = LocationRef::default();
    let metrics = skrifa_font.metrics(size, location);
    let line_height = line_spacing * font_size;
    let scale = 1.0 / metrics.units_per_em as f32 * font_size;

    for (line_num, text) in text.lines().enumerate() {
        let mut line_pen = BezPath::default();
        let mut x_offset = 0.0;

        let glyphs = shape(text, harf_font);

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

pub fn text2png(
    text: &str,
    font_size: f32,
    line_spacing: f32,
    font_bytes: &[u8],
    foreground: Color,
    background: Color,
) -> Result<Vec<u8>, EncodingError> {
    let harf_font = FontRef::new(font_bytes).expect("For font files to be font files!");
    let skrifa_font = SkrifaFontRef::new(font_bytes).expect("Fonts to be fonts");

    let expected_height = (line_spacing * font_size * text.lines().count() as f32) as f64;

    let mut path = draw_text(&harf_font, &skrifa_font, text, font_size, line_spacing).clone();
    let old_bbox = path.bounding_box();
    path.apply_affine(Affine::translate(Vec2 {
        x: -old_bbox.min_x(),
        y: -old_bbox.min_y(),
    }));
    let bbox = path.bounding_box();

    if bbox.area() == 0.0 {
        panic!("Nothing drawn (area 0). Probably unsupported glyphs.");
    }

    let y_offset = (expected_height - bbox.height()) / 2.0;

    let mut pixmap = Pixmap::new(bbox.width().ceil() as u32, expected_height as u32)
        .unwrap_or_else(|| panic!("Failed to allocate pixmap"));

    // https://github.com/linebender/tiny-skia/blob/main/examples/fill.rs basically
    pixmap.fill(background);

    let mut paint = Paint::default();
    paint.set_color(foreground);
    let path = {
        let mut pb = PathBuilder::new();
        for el in path {
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
        pb.finish().unwrap()
    };
    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::from_translate(0.0, y_offset as f32),
        None,
    );
    pixmap.encode_png()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use tiny_skia::Color;

    use crate::{testdata, text2png::text2png};

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

        assert_eq!(
            png_bytes,
            testdata::RENDER_LIGATURE_PNG,
            "Generated bytes do not match the testdata HOME_PNG bytes",
        );
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

        assert_eq!(
            png_bytes,
            testdata::RENDER_TWO_LINES_PNG,
            "Generated bytes do not match the testdata HOME_PNG bytes",
        );
    }
}
