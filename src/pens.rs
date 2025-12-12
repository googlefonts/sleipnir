//! Our own transformed bezier pen to avoid a dependency on write-fonts which is not in google3

use crate::text2png::TextToPngError;
use kurbo::{Affine, BezPath, PathEl, Point};
use skrifa::{
    color::{Brush, ColorPainter, CompositeMode, Extend, Transform},
    metrics::BoundingBox,
    outline::{DrawSettings, OutlinePen},
    prelude::{LocationRef, Size},
    raw::{tables::cpal::ColorRecord, FontRef, TableProvider},
    GlyphId, MetadataProvider, OutlineGlyphCollection,
};
use tiny_skia::{
    Color, GradientStop, LinearGradient, Paint, Point as SkiaPoint, Shader, SpreadMode,
    Transform as SkiaTransform,
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
#[derive(Debug)]
pub struct ColorFill<'a> {
    /// What to draw.
    pub paint: Paint<'a>,
    /// The path to fill.
    pub path: BezPath,
    /// The x-offset of the path.
    pub offset_x: f64,
    /// The y-offset of the path.
    pub offset_y: f64,
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
    builder: Result<ColorPainterInner<'a>, TextToPngError>,
}

struct ColorPainterInner<'a> {
    /// The path for the next fill.
    path: BezPath,
    /// All the fills that have been finalized.
    fills: Vec<ColorFill<'a>>,
}

impl<'a> ColorPainterImpl<'a> {
    /// Palette index reserved for the foreground color.
    const FOREGROUND_PALETTE_IDX: u16 = 0xFFFF;

    /// Creates a new color painter for a font.
    pub fn new(font: &FontRef<'a>, size: Size, foreground: Color, scale: f32) -> Self {
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
            builder: Ok(ColorPainterInner {
                path: BezPath::default(),
                fills: Vec::new(),
            }),
        }
    }

    /// Add a fill to the current layer, using the active path.
    pub fn add_fill(&mut self, paint: Paint<'a>) {
        if let Ok(inner) = self.builder.as_mut() {
            let path = inner.path.clone();
            let fill = ColorFill {
                paint,
                path,
                offset_x: self.x,
                offset_y: self.y,
            };
            inner.fills.push(fill);
        }
    }

    /// Returns the completed color fills, or an error if one occurred.
    pub fn fills(self) -> Result<Vec<ColorFill<'a>>, TextToPngError> {
        self.builder.map(|i| i.fills)
    }

    fn set_err(&mut self, err: TextToPngError) {
        // TODO: Consider collecting all errors instead of keeping just the first one.
        if self.builder.is_ok() {
            self.builder = Err(err);
        }
    }
}

/// Loosely based on `sk_fontations::ColorPainter`.
///
/// See <https://skia.googlesource.com/skia/+/a0fd12aac6b3/src/ports/SkTypeface_fontations_priv.h.>
impl<'a> ColorPainter for ColorPainterImpl<'a> {
    fn push_transform(&mut self, _: Transform) {
        self.set_err(TextToPngError::UnsupportedFontFeature(
            "transforms are not supported",
        ));
    }

    fn pop_transform(&mut self) {
        self.set_err(TextToPngError::UnsupportedFontFeature(
            "transforms are not supported",
        ));
    }

    fn push_clip_glyph(&mut self, glyph_id: GlyphId) {
        let Ok(inner) = self.builder.as_mut() else {
            return;
        };
        if !inner.path.is_empty() {
            self.set_err(TextToPngError::UnsupportedFontFeature(
                "multiple clip layers",
            ));
            return;
        }
        let Some(glyph) = self.outlines.get(glyph_id) else {
            self.set_err(TextToPngError::GlyphNotFound(glyph_id));
            return;
        };
        let location = LocationRef::default();
        let mut path_pen = SvgPathPen::new();
        glyph
            .draw(DrawSettings::unhinted(self.size, location), &mut path_pen)
            .unwrap();
        inner.path = path_pen.into_inner();
    }

    fn push_clip_box(&mut self, clip_box: BoundingBox) {
        let Ok(inner) = self.builder.as_mut() else {
            return;
        };
        if !inner.path.is_empty() {
            self.set_err(TextToPngError::UnsupportedFontFeature(
                "multiple clip layers",
            ));
            return;
        }
        inner.path.extend([
            PathEl::MoveTo(Point::new(clip_box.x_min as f64, clip_box.y_min as f64)),
            PathEl::LineTo(Point::new(clip_box.x_max as f64, clip_box.y_min as f64)),
            PathEl::LineTo(Point::new(clip_box.x_max as f64, clip_box.y_max as f64)),
            PathEl::LineTo(Point::new(clip_box.x_min as f64, clip_box.y_max as f64)),
            PathEl::ClosePath,
        ]);
    }

    fn pop_clip(&mut self) {
        if let Ok(inner) = self.builder.as_mut() {
            inner.path.truncate(0);
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
                        self.set_err(TextToPngError::UnsupportedFontFeature(
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
                let gradient = LinearGradient::new(
                    SkiaPoint::from_xy(p0.x, -p0.y),
                    SkiaPoint::from_xy(p1.x, -p1.y),
                    sk_color_stops,
                    spread_mode(extend),
                    SkiaTransform::from_scale(self.scale, self.scale),
                )
                .unwrap();
                Paint {
                    shader: gradient,
                    ..Paint::default()
                }
            }
            Brush::RadialGradient { .. } => {
                self.set_err(TextToPngError::UnsupportedFontFeature("radial gradients"));
                return;
            }
            Brush::SweepGradient { .. } => {
                self.set_err(TextToPngError::UnsupportedFontFeature("sweep gradients"));
                return;
            }
        };
        self.add_fill(paint);
    }

    fn push_layer(&mut self, _: CompositeMode) {
        self.set_err(TextToPngError::UnsupportedFontFeature("layers"));
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
