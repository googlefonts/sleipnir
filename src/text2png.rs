//! renders text into png, forked from <https://github.com/rsheeter/embed1/blob/main/make_test_images/src/main.rs>
use crate::{
    measure::shape,
    pens::{foreground_paint, GlyphPainter, GlyphPainterError, Paint},
};
use kurbo::{Affine, BezPath, PathEl, Rect, Shape, Vec2};
use skrifa::{
    color::{ColorPainter, Extend, PaintError},
    prelude::{LocationRef, Size},
    raw::{FontRef, ReadError},
    MetadataProvider,
};
use thiserror::Error;
use tiny_skia::{
    Color, FillRule, GradientStop, LinearGradient, Mask, Paint as SkiaPaint, PathBuilder, Pixmap,
    Point as SkiaPoint, RadialGradient, Shader, SpreadMode, Transform,
};

/// Errors encountered during the text-to-PNG rendering process.
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
    #[error("{0}")]
    GlyphPainterError(#[from] GlyphPainterError),
    #[error("Malformed gradient")]
    MalformedGradient,
}

// TODO: From<PaintError> can be autoderived with `#[from]` once
// `PaintError` implements `Error`.
impl From<PaintError> for TextToPngError {
    fn from(err: PaintError) -> TextToPngError {
        TextToPngError::PaintError(err)
    }
}

/// The fill rule used in tiny skia.
const FILL_RULE: FillRule = FillRule::Winding;

/// Returns a new Rect inflated by a margin proportional to the
/// smaller of its dimensions.
pub fn with_margin(rect: Rect, multiplier: f64) -> Rect {
    let margin = rect.width().min(rect.height()) * multiplier;
    rect.inflate(margin, margin)
}

pub struct Text2PngOptions<'a> {
    pub font_bytes: &'a [u8],
    pub font_size: f32,
    pub line_spacing: f32,
    pub foreground: Color,
    pub background: Color,
    pub location: LocationRef<'a>,
}

/// Renders a string of text into a PNG-encoded byte vector.
///
/// # Arguments
/// * `text` - The string to render.
/// * `font_size` - The size of the font in pixels.
/// * `line_spacing` - The multiplier for line height (e.g., 1.0 for single space).
/// * `font_bytes` - The raw font file data.
/// * `foreground` - The default color for non-color glyphs.
/// * `background` - The background color of the resulting PNG.
///
/// # Errors
/// Returns [`TextToPngError`] if the font is invalid, the text is empty,
/// or PNG encoding fails.
pub fn text2png(text: &str, options: Text2PngOptions) -> Result<Vec<u8>, TextToPngError> {
    let font = FontRef::new(options.font_bytes)?;
    let color_glyphs = font.color_glyphs();
    if text.split_whitespace().count() == 0 {
        return Err(TextToPngError::NoText);
    }

    let size = Size::new(options.font_size);
    let metrics = font.metrics(size, options.location);
    let line_height = options.line_spacing as f64 * options.font_size as f64;
    let scale = size.linear_scale(metrics.units_per_em);

    let mut painter = GlyphPainter::new(&font, options.location, options.foreground, size);
    for (line_num, text) in text.lines().enumerate() {
        let glyphs = shape(text, &font);
        painter.x = 0.0;
        for (glyph_info, pos) in glyphs.glyph_infos().iter().zip(glyphs.glyph_positions()) {
            // TODO: Use positions from `shape` instead of assuming left-to-right, top-to-bottom.
            painter.y = line_num as f64 * line_height;
            let glyph_id = glyph_info.glyph_id.into();
            match color_glyphs.get(glyph_id) {
                Some(color_glyph) => color_glyph.paint(options.location, &mut painter)?,
                None => {
                    painter.fill_glyph(glyph_id, None, foreground_paint());
                }
            };
            painter.x += pos.x_advance as f64 * scale as f64;
        }
    }
    let expected_height =
        (options.line_spacing * options.font_size * text.lines().count() as f32) as f64;
    let pixmap = to_pixmap(&painter.into_fills()?, options.background, expected_height)?;
    let bytes = pixmap.encode_png()?;
    Ok(bytes)
}

/// Calculates the intersection of the bounding boxes of a collection of paths.
/// Returns None if the input is empty.
fn clip_bounds(paths: &[BezPath]) -> Option<Rect> {
    paths
        .iter()
        .map(|p| p.bounding_box())
        .reduce(|a, b| a.intersect(b))
}

/// Computes the union of bounding boxes for all provided color fills,
/// considering their respective offsets and clip paths.
fn compute_bounds(fills: &[crate::pens::ColorFill]) -> Rect {
    fills
        .iter()
        .filter_map(|fill| {
            let add_offset = |b| b + Vec2::new(fill.offset_x, fill.offset_y);
            clip_bounds(&fill.clip_paths).map(add_offset)
        })
        .reduce(|a, b| a.union(b))
        .unwrap_or_default()
}

/// Create a mask from the intersection of all `paths`. If there are
/// no paths, then `None`. is returned.
fn to_mask(
    paths: &[BezPath],
    width_height: (u32, u32),
    transform: Transform,
) -> Result<Option<Mask>, TextToPngError> {
    match paths {
        [] => Ok(None),
        [path, paths @ ..] => {
            let Some(mut mask) = Mask::new(width_height.0, width_height.1) else {
                return Ok(None);
            };
            mask.fill_path(
                &path.to_tinyskia().ok_or(TextToPngError::PathBuildError)?,
                FILL_RULE,
                true,
                transform,
            );
            for path in paths {
                mask.intersect_path(
                    &path.to_tinyskia().ok_or(TextToPngError::PathBuildError)?,
                    FILL_RULE,
                    true,
                    transform,
                );
            }
            Ok(Some(mask))
        }
    }
}

/// Creates a Pixmap from a collection of color fills, centering them
/// vertically within the given height.
///
/// The Pixmap's width is determined automatically based on the
/// bounding box of the fills.
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
        let transform = Transform::from_translate(
            (fill.offset_x + x_offset) as f32,
            (fill.offset_y + y_offset) as f32,
        );
        let Some(path) = fill.clip_paths.last() else {
            continue;
        };
        let mask = to_mask(
            // OK: Guaranteed to be at least length 1 in above statement.
            &fill.clip_paths[0..fill.clip_paths.len() - 1],
            (pixmap.width(), pixmap.height()),
            transform,
        )?;
        pixmap.fill_path(
            &path.to_tinyskia().ok_or(TextToPngError::PathBuildError)?,
            &fill
                .paint
                .to_tinyskia()
                .ok_or(TextToPngError::MalformedGradient)?,
            FILL_RULE,
            transform,
            mask.as_ref(),
        );
    }
    Ok(pixmap)
}

trait ToTinySkia {
    type T;
    fn to_tinyskia(&self) -> Self::T;
}

impl ToTinySkia for BezPath {
    type T = Option<tiny_skia::Path>;

    fn to_tinyskia(&self) -> Option<tiny_skia::Path> {
        let mut pb = PathBuilder::new();
        for el in self {
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
        pb.finish()
    }
}

impl ToTinySkia for Affine {
    type T = Transform;

    fn to_tinyskia(&self) -> Transform {
        let coeffs = self.as_coeffs();
        Transform {
            sx: coeffs[0] as f32,
            ky: coeffs[1] as f32,
            kx: coeffs[2] as f32,
            sy: coeffs[3] as f32,
            tx: coeffs[4] as f32,
            ty: coeffs[5] as f32,
        }
    }
}

impl ToTinySkia for Paint {
    type T = Option<SkiaPaint<'static>>;

    fn to_tinyskia(&self) -> Option<SkiaPaint<'static>> {
        match self {
            Paint::Solid(color) => Some(SkiaPaint {
                shader: Shader::SolidColor(*color),
                ..SkiaPaint::default()
            }),
            Paint::LinearGradient {
                p0,
                p1,
                stops,
                extend,
                transform,
            } => {
                let stops = stops
                    .iter()
                    .map(|s| GradientStop::new(s.offset, s.color))
                    .collect();
                let gradient = LinearGradient::new(
                    SkiaPoint::from_xy(p0.x as f32, p0.y as f32),
                    SkiaPoint::from_xy(p1.x as f32, p1.y as f32),
                    stops,
                    extend.to_tinyskia(),
                    transform.to_tinyskia(),
                )?;
                Some(SkiaPaint {
                    shader: gradient,
                    ..SkiaPaint::default()
                })
            }
            Paint::RadialGradient {
                c0,
                // TODO: Support the full radial gradient if it becomes available in tiny_skia. At
                // the moment, we use tiny_skia's RadialGradient as an approximation for the full
                // gradient. See
                // https://github.com/linebender/tiny-skia/issues/1#issuecomment-2437703793
                r0: _,
                c1,
                r1,
                stops,
                extend,
                transform,
            } => {
                let stops = stops
                    .iter()
                    .map(|s| GradientStop::new(s.offset, s.color))
                    .collect();
                let gradient = RadialGradient::new(
                    SkiaPoint::from_xy(c0.x as f32, c0.y as f32),
                    SkiaPoint::from_xy(c1.x as f32, c1.y as f32),
                    *r1,
                    stops,
                    extend.to_tinyskia(),
                    transform.to_tinyskia(),
                )?;
                Some(SkiaPaint {
                    shader: gradient,
                    ..SkiaPaint::default()
                })
            }
        }
    }
}

impl ToTinySkia for Extend {
    type T = SpreadMode;

    fn to_tinyskia(&self) -> SpreadMode {
        match self {
            Extend::Pad => SpreadMode::Pad,
            Extend::Repeat => SpreadMode::Repeat,
            Extend::Reflect => SpreadMode::Reflect,
            // `Extend` requires non-exhaustive matching. If any new
            // variants are discovered, they should be added.
            _ => SpreadMode::Pad,
        }
    }
}

#[cfg(test)]
mod tests {
    use tiny_skia::Color;

    use crate::{
        assert_file_eq, assert_matches, testdata,
        text2png::{text2png, TextToPngError},
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
            Default::default(),
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
            Default::default(),
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
            Default::default(),
        )
        .unwrap();
        assert_file_eq!(png_bytes, "colored_font.png");
    }

    #[test]
    fn complex_emoji() {
        // TODO: Improve the centering algorithm.
        let png_bytes = text2png(
            "🥳",
            64.0,
            1.0,
            testdata::NOTO_EMOJI_FONT,
            Color::BLACK,
            Color::WHITE,
            Default::default(),
        )
        .unwrap();
        assert_file_eq!(png_bytes, "complex_emoji.png");
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
            Default::default(),
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
                Default::default(),
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
            Default::default(),
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
                Default::default(),
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
                Default::default(),
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
            Default::default(),
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
            Default::default(),
        );
        assert_matches!(result1, Err(TextToPngError::TextTooSmall));

        let result2 = text2png(
            "hello",
            12.0,
            0.0,
            testdata::CAVEAT_FONT,
            Color::WHITE,
            Color::BLACK,
            Default::default(),
        );
        assert_matches!(result2, Err(TextToPngError::TextTooSmall));
    }
}
