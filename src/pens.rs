//! Our own transformed bezier pen to avoid a dependency on write-fonts which is not in google3

use kurbo::{Affine, BezPath, PathEl, Point};
use skrifa::{
    color::{Brush, ColorPainter, CompositeMode, Extend, Transform},
    metrics::BoundingBox,
    outline::{DrawError, DrawSettings, OutlinePen},
    prelude::{LocationRef, Size},
    raw::{tables::cpal::ColorRecord, FontRef, TableProvider},
    GlyphId, MetadataProvider, OutlineGlyphCollection,
};
use thiserror::Error;
use tiny_skia::{
    Color, GradientStop, LinearGradient, Paint, Point as SkiaPoint, RadialGradient, Shader,
    SpreadMode, Transform as SkiaTransform,
};

/// Produces an svg representation of a font glyph corrected to be Y-down (as in svg) instead of Y-up (as in fonts)
pub(crate) struct SvgPathPen {
    path: BezPath,
    transform: Affine,
}

impl SvgPathPen {
    pub(crate) fn new() -> Self {
        SvgPathPen {
            path: Default::default(),
            transform: Affine::new([1.0, 0.0, 0.0, -1.0, 0.0, 0.0]),
        }
    }

    pub(crate) fn new_with_transform(transform: Affine) -> Self {
        SvgPathPen {
            path: Default::default(),
            transform,
        }
    }

    fn transform_point(&self, x: f32, y: f32) -> Point {
        self.transform * Point::new(x as f64, y as f64)
    }

    pub(crate) fn into_inner(self) -> BezPath {
        self.path
    }
}

impl OutlinePen for SvgPathPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.path.move_to(self.transform_point(x, y));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to(self.transform_point(x, y));
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.path
            .quad_to(self.transform_point(cx0, cy0), self.transform_point(x, y));
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.path.curve_to(
            self.transform_point(cx0, cy0),
            self.transform_point(cx1, cy1),
            self.transform_point(x, y),
        );
    }

    fn close(&mut self) {
        self.path.close_path();
    }
}

/// A fill produced by exercising a color glyph.
#[derive(Debug, Clone)]
pub struct ColorFill<'a> {
    /// What to draw.
    pub paint: Paint<'a>,
    /// The path to fill.
    pub clip_paths: Vec<BezPath>,
    /// The x-offset of the path.
    pub offset_x: f64,
    /// The y-offset of the path.
    pub offset_y: f64,
}

/// Error that occurs when trying to use a color painter.
#[derive(Error, Debug)]
pub enum ColorPainterError {
    #[error("glyph {0} not found")]
    GlyphNotFound(GlyphId),
    #[error("Unsupported font feature: {0}")]
    UnsupportedFontFeature(&'static str),
    #[error("Malformed gradient")]
    MalformedGradient,
    #[error("{0}")]
    DrawError(#[from] DrawError),
}

/// A [ColorPainter] that generates a series of [ColorFill]s.
pub struct ColorPainterImpl<'a> {
    /// The x-offset for the next fill operation.
    pub x: f64,
    /// The y-offset for the next fill operation.
    pub y: f64,
    size: Size,
    scale: f32,
    outlines: OutlineGlyphCollection<'a>,
    foreground: Color,
    colors: &'a [ColorRecord],
    builder: Result<ColorFillsBuilder<'a>, ColorPainterError>,
}

struct ColorFillsBuilder<'a> {
    /// The path for the next fill.
    paths: Vec<BezPath>,
    transforms: Vec<Affine>,
    /// All the fills that have been finalized.
    fills: Vec<ColorFill<'a>>,
}

/// TODO: Make this into a const once <https://github.com/googlefonts/fontations/pull/1707> has been
/// released.
pub const fn foreground_paint() -> skrifa::color::Brush<'static> {
    skrifa::color::Brush::Solid {
        palette_index: ColorPainterImpl::FOREGROUND_PALETTE_IDX,
        alpha: 1.0,
    }
}

impl<'a> ColorPainterImpl<'a> {
    /// Palette index reserved for the foreground color.
    const FOREGROUND_PALETTE_IDX: u16 = 0xFFFF;

    /// Creates a new color painter for a font.
    pub fn new(font: &FontRef<'a>, foreground: Color, size: Size) -> Self {
        let upem = font.head().map(|h| h.units_per_em());
        let scale = upem.map(|upem| size.linear_scale(upem)).unwrap_or(1.0);
        let outlines = font.outline_glyphs();
        let colors = match font.cpal().map(|c| c.color_records_array()) {
            Ok(Some(Ok(c))) => c,
            _ => &[],
        };
        ColorPainterImpl {
            x: 0.0,
            y: 0.0,
            size,
            scale,
            outlines,
            foreground,
            colors,
            builder: Ok(ColorFillsBuilder {
                paths: Vec::new(),
                transforms: Vec::new(),
                fills: Vec::new(),
            }),
        }
    }

    /// Returns the completed color fills, or an error if one occurred.
    pub fn into_fills(self) -> Result<Vec<ColorFill<'a>>, ColorPainterError> {
        self.builder.map(|i| i.fills)
    }

    fn set_err(&mut self, err: ColorPainterError) {
        // TODO: Consider collecting all errors instead of keeping just the first one.
        if self.builder.is_ok() {
            self.builder = Err(err);
        }
    }
}

impl<'a> ColorFillsBuilder<'a> {
    fn current_transform(&self) -> Affine {
        self.transforms.last().copied().unwrap_or_default()
    }
}

/// Loosely based on `sk_fontations::ColorPainter`.
///
/// See <https://skia.googlesource.com/skia/+/a0fd12aac6b3/src/ports/SkTypeface_fontations_priv.h.>
/// for another example implementation of `ColorPainter`.
impl<'a> ColorPainter for ColorPainterImpl<'a> {
    fn push_transform(&mut self, transform: Transform) {
        let Ok(builder) = self.builder.as_mut() else {
            return;
        };
        let transform = Affine::new([
            transform.xx as f64,
            transform.yx as f64,
            transform.xy as f64,
            transform.yy as f64,
            transform.dx as f64,
            transform.dy as f64,
        ]);
        let new_transform = match builder.transforms.last().copied() {
            Some(prev_transform) => transform * prev_transform,
            None => transform,
        };
        builder.transforms.push(new_transform);
    }

    fn pop_transform(&mut self) {
        let Ok(builder) = self.builder.as_mut() else {
            return;
        };
        builder.transforms.pop();
    }

    fn push_clip_glyph(&mut self, glyph_id: GlyphId) {
        let Ok(builder) = self.builder.as_mut() else {
            return;
        };
        let Some(glyph) = self.outlines.get(glyph_id) else {
            self.set_err(ColorPainterError::GlyphNotFound(glyph_id));
            return;
        };

        let location = LocationRef::default();
        let transform = builder.current_transform();
        let (pen_transform, draw_settings) = if self.colors.is_empty() {
            (
                Affine::scale_non_uniform(1.0, -1.0) * transform,
                DrawSettings::unhinted(self.size, location),
            )
        } else {
            // TODO: Colored fonts should use the same transform as non-colored. You can observe
            // misplaced glyphs in the `complex_emoji` test in src/text2png.rs
            (
                Affine::scale_non_uniform(self.scale as f64, -self.scale as f64) * transform,
                DrawSettings::unhinted(Size::unscaled(), location),
            )
        };
        let mut path_pen = SvgPathPen::new_with_transform(pen_transform);
        match glyph.draw(draw_settings, &mut path_pen) {
            Ok(_) => builder.paths.push(path_pen.into_inner()),
            Err(err) => {
                self.set_err(err.into());
            }
        }
    }

    fn push_clip_box(&mut self, clip_box: BoundingBox) {
        let Ok(builder) = self.builder.as_mut() else {
            return;
        };
        let path = BezPath::from_vec(vec![
            PathEl::MoveTo(Point::new(clip_box.x_min as f64, clip_box.y_min as f64)),
            PathEl::LineTo(Point::new(clip_box.x_max as f64, clip_box.y_min as f64)),
            PathEl::LineTo(Point::new(clip_box.x_max as f64, clip_box.y_max as f64)),
            PathEl::LineTo(Point::new(clip_box.x_min as f64, clip_box.y_max as f64)),
            PathEl::ClosePath,
        ]);
        let transform = Affine::scale_non_uniform(self.scale as f64, -self.scale as f64)
            * builder.current_transform();
        builder.paths.push(transform * path);
    }

    fn pop_clip(&mut self) {
        if let Ok(builder) = self.builder.as_mut() {
            builder.paths.pop();
        }
    }

    fn fill(&mut self, brush: Brush<'_>) {
        macro_rules! color_or_exit {
            ($palette_idx:expr, $alpha:expr) => {
                if $palette_idx == Self::FOREGROUND_PALETTE_IDX {
                    let mut color = self.foreground;
                    color.set_alpha($alpha);
                    color
                } else {
                    let Some(color) = self.colors.get($palette_idx as usize) else {
                        self.set_err(ColorPainterError::UnsupportedFontFeature(
                            "color palette index out of bounds",
                        ));
                        return;
                    };

                    let max = u8::MAX as f32;
                    Color::from_rgba8(
                        color.red,
                        color.green,
                        color.blue,
                        ($alpha * max).clamp(0.0, max) as u8,
                    )
                }
            };
        }

        let Ok(builder) = self.builder.as_mut() else {
            return;
        };
        let transform = builder.current_transform();
        let paint = match brush {
            Brush::Solid {
                palette_index,
                alpha,
            } => Paint {
                shader: Shader::SolidColor(color_or_exit!(palette_index, alpha)),
                ..Paint::default()
            },
            Brush::LinearGradient {
                p0,
                p1,
                color_stops,
                extend,
            } => {
                let mut sk_color_stops = Vec::with_capacity(color_stops.len());
                for stop in color_stops.iter() {
                    sk_color_stops.push(GradientStop::new(
                        stop.offset,
                        color_or_exit!(stop.palette_index, stop.alpha),
                    ));
                }
                let p0 = transform * Point::new(p0.x as f64, p0.y as f64);
                let p1 = transform * Point::new(p1.x as f64, p1.y as f64);
                let Some(gradient) = LinearGradient::new(
                    SkiaPoint::from_xy(p0.x as f32, -p0.y as f32),
                    SkiaPoint::from_xy(p1.x as f32, -p1.y as f32),
                    sk_color_stops,
                    spread_mode(extend),
                    SkiaTransform::from_scale(self.scale, self.scale),
                ) else {
                    self.set_err(ColorPainterError::MalformedGradient);
                    return;
                };
                Paint {
                    shader: gradient,
                    ..Paint::default()
                }
            }
            Brush::RadialGradient {
                c0,
                r0,
                c1,
                r1,
                color_stops,
                extend,
            } => {
                let mut sk_color_stops = Vec::with_capacity(color_stops.len());
                for stop in color_stops.iter() {
                    sk_color_stops.push(GradientStop::new(
                        stop.offset,
                        color_or_exit!(stop.palette_index, stop.alpha),
                    ));
                }
                let c0 = transform * Point::new(c0.x as f64, c0.y as f64);
                let c1 = transform * Point::new(c1.x as f64, c1.y as f64);
                // TODO: Support the full radial gradient if it
                // becomes available in tiny_skia. At the moment, we
                // use tiny_skia's RadialGradient as an approximation
                // for the full gradient. See
                // https://github.com/linebender/tiny-skia/issues/1#issuecomment-2437703793
                let _ = r0;
                let Some(gradient) = RadialGradient::new(
                    SkiaPoint::from_xy(c0.x as f32, -c0.y as f32),
                    SkiaPoint::from_xy(c1.x as f32, -c1.y as f32),
                    r1,
                    sk_color_stops,
                    spread_mode(extend),
                    SkiaTransform::from_scale(self.scale, self.scale),
                ) else {
                    self.set_err(ColorPainterError::MalformedGradient);
                    return;
                };
                Paint {
                    shader: gradient,
                    ..Paint::default()
                }
            }
            Brush::SweepGradient { .. } => {
                self.set_err(ColorPainterError::UnsupportedFontFeature(
                    "colr sweep gradients",
                ));
                return;
            }
        };
        builder.fills.push(ColorFill {
            paint,
            clip_paths: builder.paths.clone(),
            offset_x: self.x,
            offset_y: self.y,
        });
    }

    fn push_layer(&mut self, _: CompositeMode) {
        self.set_err(ColorPainterError::UnsupportedFontFeature("colr layers"));
    }
}

fn spread_mode(extend: Extend) -> SpreadMode {
    match extend {
        Extend::Pad => SpreadMode::Pad,
        Extend::Repeat => SpreadMode::Repeat,
        Extend::Reflect => SpreadMode::Reflect,
        // `Extend` requires non-exhaustive matching. If any new
        // variants are discovered, they should be added.
        _ => SpreadMode::Pad,
    }
}
