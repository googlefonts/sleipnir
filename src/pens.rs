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
pub enum GlyphPainterError {
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
pub struct GlyphPainter<'a> {
    /// The x-offset for the next fill operation.
    pub x: f64,
    /// The y-offset for the next fill operation.
    pub y: f64,
    location: LocationRef<'a>,
    size: Size,
    scale: f32,
    outlines: OutlineGlyphCollection<'a>,
    foreground: Color,
    is_colr: bool,
    colors: &'a [ColorRecord],
    builder: Result<ColorFillsBuilder<'a>, GlyphPainterError>,
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
        palette_index: GlyphPainter::FOREGROUND_PALETTE_IDX,
        alpha: 1.0,
    }
}

impl<'a> GlyphPainter<'a> {
    /// Palette index reserved for the foreground color.
    const FOREGROUND_PALETTE_IDX: u16 = 0xFFFF;

    /// Creates a new color painter for a font.
    pub fn new(
        font: &FontRef<'a>,
        location: LocationRef<'a>,
        foreground: Color,
        size: Size,
    ) -> Self {
        let upem = font.head().map(|h| h.units_per_em());
        let scale = upem.map(|upem| size.linear_scale(upem)).unwrap_or(1.0);
        let outlines = font.outline_glyphs();
        let is_colr = font.colr().is_ok();
        let colors = match font.cpal().map(|c| c.color_records_array()) {
            Ok(Some(Ok(c))) => c,
            _ => &[],
        };
        GlyphPainter {
            x: 0.0,
            y: 0.0,
            location,
            size,
            scale,
            outlines,
            foreground,
            is_colr,
            colors,
            builder: Ok(ColorFillsBuilder {
                paths: Vec::new(),
                transforms: Vec::new(),
                fills: Vec::new(),
            }),
        }
    }

    /// Returns the completed color fills, or an error if one occurred.
    pub fn into_fills(self) -> Result<Vec<ColorFill<'a>>, GlyphPainterError> {
        self.builder.map(|i| i.fills)
    }

    fn set_err(&mut self, err: GlyphPainterError) {
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
impl<'a> ColorPainter for GlyphPainter<'a> {
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
            self.set_err(GlyphPainterError::GlyphNotFound(glyph_id));
            return;
        };

        let (size, scale) = if self.is_colr {
            // colr may define transformations which should be applied before scaling. We accomplish
            // this by drawing unscaled and applying the scaling after.
            (Size::unscaled(), self.scale as f64)
        } else {
            (self.size, 1.0)
        };
        let draw_settings = DrawSettings::unhinted(size, self.location);
        let mut path_pen = SvgPathPen::new_with_transform(
            builder
                .current_transform()
                .then_scale_non_uniform(scale, -scale),
        );
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
        let transform = builder
            .current_transform()
            .then_scale_non_uniform(self.scale as f64, -self.scale as f64);
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
                        self.set_err(GlyphPainterError::UnsupportedFontFeature(
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
        let transform = builder
            .current_transform()
            .then_scale_non_uniform(self.scale as f64, -self.scale as f64);
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
                let Some(gradient) = LinearGradient::new(
                    SkiaPoint::from_xy(p0.x, p0.y),
                    SkiaPoint::from_xy(p1.x, p1.y),
                    sk_color_stops,
                    spread_mode(extend),
                    SkiaTransform {
                        sx: transform.as_coeffs()[0] as f32,
                        ky: transform.as_coeffs()[1] as f32,
                        kx: transform.as_coeffs()[2] as f32,
                        sy: transform.as_coeffs()[3] as f32,
                        tx: transform.as_coeffs()[4] as f32,
                        ty: transform.as_coeffs()[5] as f32,
                    },
                ) else {
                    self.set_err(GlyphPainterError::MalformedGradient);
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
                // TODO: Support the full radial gradient if it
                // becomes available in tiny_skia. At the moment, we
                // use tiny_skia's RadialGradient as an approximation
                // for the full gradient. See
                // https://github.com/linebender/tiny-skia/issues/1#issuecomment-2437703793
                let _ = r0;
                let Some(gradient) = RadialGradient::new(
                    SkiaPoint::from_xy(c0.x, c0.y),
                    SkiaPoint::from_xy(c1.x, c1.y),
                    r1,
                    sk_color_stops,
                    spread_mode(extend),
                    SkiaTransform {
                        sx: transform.as_coeffs()[0] as f32,
                        ky: transform.as_coeffs()[1] as f32,
                        kx: transform.as_coeffs()[2] as f32,
                        sy: transform.as_coeffs()[3] as f32,
                        tx: transform.as_coeffs()[4] as f32,
                        ty: transform.as_coeffs()[5] as f32,
                    },
                ) else {
                    self.set_err(GlyphPainterError::MalformedGradient);
                    return;
                };
                Paint {
                    shader: gradient,
                    ..Paint::default()
                }
            }
            Brush::SweepGradient { .. } => {
                self.set_err(GlyphPainterError::UnsupportedFontFeature(
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
        self.set_err(GlyphPainterError::UnsupportedFontFeature("colr layers"));
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
