//! Our own transformed bezier pen to avoid a dependency on write-fonts which is not in google3

use kurbo::{BezPath, Point};
use skrifa::outline::OutlinePen;

/// Produces an svg representation of a font glyph corrected to be Y-down (as in svg) instead of Y-up (as in fonts)
pub(crate) struct SvgPathPen {
    path: BezPath,
}

impl SvgPathPen {
    pub(crate) fn new() -> Self {
        Self {
            path: Default::default(),
        }
    }

    fn to_svg_units(&self, x: f32, y: f32) -> Point {
        // svg is Y-down, fonts are Y-up
        Point::new(x as f64, -y as f64)
    }

    pub(crate) fn into_inner(self) -> BezPath {
        self.path
    }
}

impl OutlinePen for SvgPathPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.path.move_to(self.to_svg_units(x, y));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to(self.to_svg_units(x, y));
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.path
            .quad_to(self.to_svg_units(cx0, cy0), self.to_svg_units(x, y));
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.path.curve_to(
            self.to_svg_units(cx0, cy0),
            self.to_svg_units(cx1, cy1),
            self.to_svg_units(x, y),
        );
    }

    fn close(&mut self) {
        self.path.close_path();
    }
}
