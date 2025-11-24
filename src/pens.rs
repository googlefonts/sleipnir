//! Our own transformed bezier pen to avoid a dependency on write-fonts which is not in google3

use kurbo::{Affine, BezPath, PathEl, Point};
use skrifa::outline::OutlinePen;

/// Produces an svg representation of a font glyph corrected to be Y-down (as in svg) instead of Y-up (as in fonts)
pub(crate) struct SvgPathPen {
    path: BezPath,
    transform: Affine,
}

impl SvgPathPen {
    pub(crate) fn new() -> Self {
        SvgPathPen::new_with_transform(Affine::new([1.0, 0.0, 0.0, -1.0, 0.0, 0.0]))
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

pub(crate) struct PathVisitor<F> {
    f: F,
}

impl<F> PathVisitor<F> {
    pub fn new(f: F) -> Self {
        Self { f }
    }
}

impl<F: FnMut(PathEl)> OutlinePen for PathVisitor<F> {
    fn move_to(&mut self, x: f32, y: f32) {
        (self.f)(kurbo::PathEl::MoveTo((x, y).into()));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        (self.f)(kurbo::PathEl::LineTo((x, y).into()));
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        (self.f)(kurbo::PathEl::QuadTo((cx0, cy0).into(), (x, y).into()));
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let el = kurbo::PathEl::CurveTo((cx0, cy0).into(), (cx1, cy1).into(), (x, y).into());
        (self.f)(el);
    }

    fn close(&mut self) {
        (self.f)(kurbo::PathEl::ClosePath);
    }
}
