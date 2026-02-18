//! renders text into png, forked from <https://github.com/rsheeter/embed1/blob/main/make_test_images/src/main.rs>
use crate::{
    measure::shape,
    pens::{foreground_paint, DrawItem, GlyphPainter, GlyphPainterError, Paint},
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
    Point as SkiaPoint, RadialGradient, Shader, SpreadMode, SweepGradient, Transform,
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

/// Options for rendering text to PNG.
#[derive(Debug, Clone)]
pub struct Text2PngOptions<'a> {
    /// The raw font file data.
    pub font_bytes: &'a [u8],
    /// The size of the font in pixels.
    pub font_size: f32,
    /// The multiplier for the font size to determine line height (e.g., 1.0 means line height
    /// equals font size).
    pub line_spacing: f32,
    /// The default color for non-color glyphs.
    pub foreground: Color,
    /// The background color of the resulting PNG.
    pub background: Color,
    /// The font variations and settings.
    pub location: LocationRef<'a>,
}

impl<'a> Text2PngOptions<'a> {
    /// Creates a new set of options with default values.
    ///
    /// # Example
    /// ```
    /// # use tiny_skia::Color;
    /// # use sleipnir::text2png::Text2PngOptions;
    /// # let font_bytes = Vec::new();
    /// let options = Text2PngOptions {
    ///     foreground: Color::from_rgba8(255, 0, 0, 255),
    ///     ..Text2PngOptions::new(&font_bytes, 24.0)
    /// };
    /// ```
    pub fn new(font_bytes: &'a [u8], font_size: f32) -> Self {
        Self {
            font_bytes,
            font_size,
            line_spacing: 1.0,
            foreground: Color::BLACK,
            background: Color::TRANSPARENT,
            location: LocationRef::default(),
        }
    }
}

/// Renders a string of text into a PNG-encoded byte vector.
///
/// # Arguments
/// * `text` - The string to render.
/// * `options` - Configuration for the rendering process.
///
/// # Errors
/// Returns [`TextToPngError`] if the font is invalid, the text is empty,
/// or PNG encoding fails.
pub fn text2png(text: &str, options: &Text2PngOptions) -> Result<Vec<u8>, TextToPngError> {
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
        let glyphs = shape(text, &font, options.location);
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
    let pixmap = to_pixmap(&painter.into_items()?, options.background, expected_height)?;
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

/// Computes the union of bounding boxes for all provided draw items,
/// considering their respective offsets and clip paths.
fn compute_bounds(items: &[DrawItem]) -> Rect {
    items
        .iter()
        .filter_map(|item| match item {
            DrawItem::Fill(fill) => {
                let add_offset = |b| b + Vec2::new(fill.offset_x, fill.offset_y);
                clip_bounds(&fill.clip_paths).map(add_offset)
            }
            DrawItem::Layer(layer) => {
                let b = compute_bounds(&layer.items);
                if b == Rect::default() {
                    None
                } else {
                    Some(b)
                }
            }
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

fn render_items(
    items: &[DrawItem],
    pixmap: &mut Pixmap,
    x_offset: f64,
    y_offset: f64,
) -> Result<(), TextToPngError> {
    for item in items {
        match item {
            DrawItem::Fill(fill) => {
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
            DrawItem::Layer(layer) => render_items(&layer.items, pixmap, x_offset, y_offset)?,
        }
    }
    Ok(())
}

/// Creates a Pixmap from a collection of draw items, centering them
/// vertically within the given height.
///
/// The Pixmap's width is determined automatically based on the
/// bounding box of the items.
fn to_pixmap(
    items: &[DrawItem],
    background: Color,
    height: f64,
) -> Result<Pixmap, TextToPngError> {
    let bounds = compute_bounds(items);
    let width = bounds.width();

    let mut pixmap = Pixmap::new(width.ceil() as u32, height.ceil() as u32)
        .ok_or(TextToPngError::TextTooSmall)?;
    pixmap.fill(background);
    let x_offset = -bounds.min_x();
    let y_offset_for_centering = (height - bounds.height()) / 2.0;
    let y_offset = y_offset_for_centering - bounds.min_y();
    render_items(items, &mut pixmap, x_offset, y_offset)?;
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
        let shader = match self {
            Paint::Solid(color) => Shader::SolidColor(*color),
            Paint::LinearGradient {
                p0,
                p1,
                stops,
                extend,
                transform,
            } => LinearGradient::new(
                p0.to_tinyskia(),
                p1.to_tinyskia(),
                stops.to_tinyskia(),
                extend.to_tinyskia(),
                transform.to_tinyskia(),
            )?,
            Paint::RadialGradient {
                c0,
                r0,
                c1,
                r1,
                stops,
                extend,
                transform,
            } => RadialGradient::new(
                c0.to_tinyskia(),
                *r0,
                c1.to_tinyskia(),
                *r1,
                stops.to_tinyskia(),
                extend.to_tinyskia(),
                transform.to_tinyskia(),
            )?,
            Paint::SweepGradient {
                c0,
                start_angle,
                end_angle,
                stops,
                extend,
                transform,
            } => SweepGradient::new(
                c0.to_tinyskia(),
                *start_angle,
                *end_angle,
                stops.to_tinyskia(),
                extend.to_tinyskia(),
                transform.to_tinyskia(),
            )?,
        };
        Some(SkiaPaint {
            shader,
            ..SkiaPaint::default()
        })
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

impl ToTinySkia for Vec<crate::pens::ColorStop> {
    type T = Vec<GradientStop>;

    fn to_tinyskia(&self) -> Vec<GradientStop> {
        self.iter()
            .map(|s| GradientStop::new(s.offset, s.color))
            .collect()
    }
}

impl ToTinySkia for kurbo::Point {
    type T = SkiaPoint;

    fn to_tinyskia(&self) -> SkiaPoint {
        SkiaPoint::from_xy(self.x as f32, self.y as f32)
    }
}

#[cfg(test)]
mod tests {
    use skrifa::{FontRef, MetadataProvider};
    use tiny_skia::{Color, Pixmap};

    use crate::{
        assert_file_eq, assert_matches, testdata,
        text2png::{text2png, Text2PngOptions, TextToPngError},
    };

    #[test]
    fn ligature() {
        let png_bytes = text2png(
            "fitto",
            &Text2PngOptions {
                foreground: Color::from_rgba8(255, 255, 255, 255),
                background: Color::from_rgba8(20, 20, 20, 255),
                ..Text2PngOptions::new(testdata::CAVEAT_FONT, 24.0)
            },
        )
        .expect("To draw PNG");

        assert_file_eq!(png_bytes, "render_ligature.png");
    }

    #[test]
    fn two_lines() {
        let png_bytes = text2png(
            "hello\nworld",
            &Text2PngOptions {
                foreground: Color::from_rgba8(255, 255, 255, 255),
                background: Color::from_rgba8(20, 20, 20, 255),
                ..Text2PngOptions::new(testdata::CAVEAT_FONT, 24.0)
            },
        )
        .expect("To draw PNG");

        assert_file_eq!(png_bytes, "render_two_lines.png");
    }

    #[test]
    fn colored_font() {
        let png_bytes = text2png(
            "abab\nABAB",
            &Text2PngOptions {
                background: Color::WHITE,
                ..Text2PngOptions::new(testdata::NABLA_FONT, 64.0)
            },
        )
        .unwrap();
        assert_file_eq!(png_bytes, "colored_font.png");
    }

    #[test]
    fn sweep_gradient() {
        let sweep_gradient_text ="\u{f0200}\u{f0201}\u{f0202}\u{f0203}\u{f0204}\u{f0205}\u{f0206}\u{f0207}\u{f0208}\u{f0209}\u{f020a}\u{f020b}\u{f020c}\u{f020d}\u{f020e}\u{f020f}\n\u{f0210}\u{f0211}\u{f0212}\u{f0213}\u{f0214}\u{f0215}\u{f0216}\u{f0217}\u{f0218}\u{f0219}\u{f021a}\u{f021b}\u{f021c}\u{f021d}\u{f021e}\u{f021f}\n\u{f0220}\u{f0221}\u{f0222}\u{f0223}\u{f0224}\u{f0225}\u{f0226}\u{f0227}\u{f0228}\u{f0229}\u{f022a}\u{f022b}\u{f022c}\u{f022d}\u{f022e}\u{f022f}\n\u{f0230}\u{f0231}\u{f0232}\u{f0233}\u{f0234}\u{f0235}\u{f0236}\u{f0237}\u{f0238}\u{f0239}\u{f023a}\u{f023b}\u{f023c}\u{f023d}\u{f023e}\u{f023f}\n\u{f0240}\u{f0241}\u{f0242}\u{f0243}\u{f0244}\u{f0245}\u{f0246}\u{f0247}";
        let png_bytes = text2png(
            sweep_gradient_text,
            &Text2PngOptions::new(testdata::COLR_FONT, 64.0),
        )
        .unwrap();
        assert_file_eq!(png_bytes, "sweep_gradient.png");
    }

    #[test]
    fn complex_emoji() {
        // TODO: Improve the centering algorithm.
        let png_bytes = text2png(
            "🥳",
            &Text2PngOptions {
                background: Color::WHITE,
                ..Text2PngOptions::new(testdata::NOTO_EMOJI_FONT, 64.0)
            },
        )
        .unwrap();
        assert_file_eq!(png_bytes, "complex_emoji.png");
    }

    #[test]
    fn empty_string_produces_error() {
        assert_matches!(
            text2png("", &Text2PngOptions::new(testdata::CAVEAT_FONT, 24.0)),
            Err(TextToPngError::NoText)
        );
    }

    #[test]
    fn unmapped_character_produces_error() {
        assert_matches!(
            text2png(
                // "c" is not included in our subsetted NABLA_FONT used for testing.
                "c",
                &Text2PngOptions::new(testdata::NABLA_FONT, 64.0),
            ),
            // TODO: Produce a better error.
            Err(TextToPngError::PathBuildError)
        );
    }

    #[test]
    fn whitespace_only_produces_error() {
        assert_matches!(
            text2png("\n \n", &Text2PngOptions::new(testdata::CAVEAT_FONT, 24.0)),
            Err(TextToPngError::NoText)
        );

        assert_matches!(
            text2png("\r", &Text2PngOptions::new(testdata::CAVEAT_FONT, 24.0)),
            Err(TextToPngError::NoText)
        );
        assert_matches!(
            text2png("\t", &Text2PngOptions::new(testdata::CAVEAT_FONT, 24.0)),
            Err(TextToPngError::NoText)
        );
    }

    #[test]
    fn bad_font_data_produces_error() {
        let bad_font_data = &[];
        assert_matches!(
            text2png("hello world", &Text2PngOptions::new(bad_font_data, 24.0)),
            Err(TextToPngError::ReadError(_))
        );
    }

    #[test]
    fn zero_size_font_produces_error() {
        assert_matches!(
            text2png("hello", &Text2PngOptions::new(testdata::CAVEAT_FONT, 0.0)),
            Err(TextToPngError::TextTooSmall)
        );

        assert_matches!(
            text2png(
                "hello",
                &Text2PngOptions {
                    line_spacing: 0.0,
                    ..Text2PngOptions::new(testdata::CAVEAT_FONT, 12.0)
                },
            ),
            Err(TextToPngError::TextTooSmall)
        );
    }

    fn active_pixels(pixmap: &Pixmap) -> f64 {
        let active_sum = pixmap
            .pixels()
            .iter()
            .map(|pixel| pixel.alpha() as u64)
            .sum::<u64>();
        active_sum as f64 / 255.0
    }

    #[test]
    fn variable_font() {
        let default = Pixmap::decode_png(
            &text2png(
                "AA",
                &Text2PngOptions::new(testdata::INCONSOLATA_FONT, 24.0),
            )
            .unwrap(),
        )
        .unwrap();
        let wide_heavy = Pixmap::decode_png(
            &text2png(
                "AA",
                &Text2PngOptions {
                    location: (&FontRef::new(testdata::INCONSOLATA_FONT)
                        .unwrap()
                        .axes()
                        .location([("wght", 900.0), ("wdth", 200.0)]))
                        .into(),
                    ..Text2PngOptions::new(testdata::INCONSOLATA_FONT, 24.0)
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert!(
            default.width() < wide_heavy.width(),
            "{} < {}",
            default.width(),
            wide_heavy.width()
        );

        assert!(
            active_pixels(&default) < active_pixels(&wide_heavy),
            "{} < {}",
            active_pixels(&default),
            active_pixels(&wide_heavy)
        );
    }
}
