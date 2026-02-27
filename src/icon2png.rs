//! Renders a single icon glyph to PNG using the tiny_skia pipeline.
use crate::{
    draw_glyph::DrawOptions,
    error::IconResolutionError,
    icon2svg::color_from_u32,
    iconid::IconIdentifier,
    pens::{foreground_paint, ColorFill, GlyphPainter, GlyphPainterError, Paint},
};
use kurbo::{Affine, BezPath, PathEl, Rect, Shape, Vec2};
use skrifa::{
    color::{ColorPainter, Extend, PaintError},
    prelude::Size,
    raw::FontRef,
    MetadataProvider,
};
use thiserror::Error;
use tiny_skia::{
    Color, FillRule, GradientStop, LinearGradient, Mask, Paint as SkiaPaint, PathBuilder, Pixmap,
    Point as SkiaPoint, RadialGradient, Shader, SpreadMode, SweepGradient, Transform,
};

/// Errors encountered during icon-to-PNG rendering.
#[derive(Error, Debug)]
pub enum Icon2PngError {
    #[error("Unable to determine glyph id for {0:?}: {1}")]
    ResolutionError(IconIdentifier, IconResolutionError),
    #[error("{0}")]
    PaintError(PaintError),
    #[error("{0}")]
    GlyphPainterError(#[from] GlyphPainterError),
    #[error("Failed to build render path")]
    PathBuildError,
    #[error("the icon was too small to render")]
    TooSmall,
    #[error("Malformed gradient")]
    MalformedGradient,
    #[error("error encoding bitmap to png: {0}")]
    PngEncodingError(#[from] png::EncodingError),
}

// TODO: From<PaintError> can be autoderived with `#[from]` once
// `PaintError` implements `Error`.
impl From<PaintError> for Icon2PngError {
    fn from(err: PaintError) -> Icon2PngError {
        Icon2PngError::PaintError(err)
    }
}

/// The fill rule used in tiny skia.
const FILL_RULE: FillRule = FillRule::EvenOdd;

/// Renders a single icon glyph from a font to a PNG-encoded byte vector.
///
/// The icon is rendered into a square pixmap of `options.width_height × options.width_height`
/// pixels, centered both horizontally and vertically. The background is transparent.
///
/// Supports both outline glyphs and COLR color glyphs.
///
/// # Errors
/// Returns [`Icon2PngError`] if the glyph cannot be resolved, painting fails,
/// or PNG encoding fails.
pub fn icon2png(font: &FontRef, options: &DrawOptions) -> Result<Vec<u8>, Icon2PngError> {
    let gid = options
        .identifier
        .resolve(font, options.location)
        .map_err(|e| Icon2PngError::ResolutionError(options.identifier.clone(), e))?;

    let foreground = options
        .fill_color
        .map(color_from_u32)
        .unwrap_or(Color::BLACK);

    let size = Size::new(options.width_height);
    let mut painter = GlyphPainter::new(font, options.location, foreground, size);

    match font.color_glyphs().get(gid) {
        Some(color_glyph) => color_glyph.paint(options.location, &mut painter)?,
        None => {
            painter.fill_glyph(gid, None, foreground_paint());
        }
    };

    let fills = painter.into_fills()?;
    let size_px = options.width_height.ceil() as u32;
    let pixmap = icon_to_pixmap(&fills, Color::TRANSPARENT, size_px)?;
    let png = pixmap.encode_png().unwrap();
    Ok(png)
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
fn compute_bounds(fills: &[ColorFill]) -> Rect {
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
/// no paths, then `None` is returned.
fn to_mask(
    paths: &[BezPath],
    width_height: (u32, u32),
    transform: Transform,
) -> Result<Option<Mask>, Icon2PngError> {
    match paths {
        [] => Ok(None),
        [path, paths @ ..] => {
            let Some(mut mask) = Mask::new(width_height.0, width_height.1) else {
                return Ok(None);
            };
            mask.fill_path(
                &path.to_tinyskia().ok_or(Icon2PngError::PathBuildError)?,
                FILL_RULE,
                true,
                transform,
            );
            for path in paths {
                mask.intersect_path(
                    &path.to_tinyskia().ok_or(Icon2PngError::PathBuildError)?,
                    FILL_RULE,
                    true,
                    transform,
                );
            }
            Ok(Some(mask))
        }
    }
}

/// Creates a square Pixmap of the given `size`, renders the fills into it centered
/// both horizontally and vertically.
fn icon_to_pixmap(
    fills: &[ColorFill],
    background: Color,
    size: u32,
) -> Result<Pixmap, Icon2PngError> {
    let bounds = compute_bounds(fills);

    let mut pixmap = Pixmap::new(size, size).ok_or(Icon2PngError::TooSmall)?;
    if background.alpha() > 0.0 {
        pixmap.fill(background);
    }

    let x_offset = (size as f64 - bounds.width()) / 2.0 - bounds.min_x();
    let y_offset = (size as f64 - bounds.height()) / 2.0 - bounds.min_y();

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
            &path.to_tinyskia().ok_or(Icon2PngError::PathBuildError)?,
            &fill
                .paint
                .to_tinyskia()
                .ok_or(Icon2PngError::MalformedGradient)?,
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
    use skrifa::{prelude::LocationRef, FontRef, MetadataProvider};

    use crate::{
        assert_file_eq,
        draw_glyph::DrawOptions,
        icon2png::icon2png,
        iconid::{self, IconIdentifier},
        pathstyle::SvgPathStyle,
        testdata,
    };

    fn test_options(identifier: IconIdentifier) -> DrawOptions<'static> {
        DrawOptions::new(
            identifier,
            64.0,
            LocationRef::default(),
            SvgPathStyle::Unchanged(2),
        )
    }

    #[test]
    fn draw_simple_icon() {
        let font = FontRef::new(testdata::ICON_FONT).unwrap();
        let loc = font.axes().location(&[
            ("wght", 400.0),
            ("opsz", 24.0),
            ("GRAD", 0.0),
            ("FILL", 1.0),
        ]);
        let options = DrawOptions {
            location: (&loc).into(),
            ..test_options(iconid::MAIL.clone())
        };
        let result = icon2png(&font, &options).expect("To draw PNG");
        assert_file_eq!(result, "mail_icon.png");
    }

    #[test]
    fn draw_color_icon() {
        let font = FontRef::new(testdata::NOTO_EMOJI_FONT).unwrap();
        let options = test_options(IconIdentifier::Codepoint('🥳' as u32));
        let result = icon2png(&font, &options).expect("To draw PNG");
        assert_file_eq!(result, "color_icon.png");
    }
}
