//! Our own transformed bezier pen to avoid a dependency on write-fonts which is not in google3

use kurbo::{Affine, BezPath, PathEl, Point};
use skrifa::outline::OutlinePen;
use std::fmt::Write;

/// Produces an svg representation of a font glyph corrected to be Y-down (as in svg) instead of Y-up (as in fonts)
pub(crate) struct SvgPathPen {
    transform: Affine,
    path: BezPath,
}

fn _round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}
fn push_point(svg: &mut String, prefix: char, p: Point) {
    svg.push(prefix);
    write!(svg, "{},{}", _round2(p.x), _round2(p.y)).expect("We can't write into a String?!");
}

impl SvgPathPen {
    pub(crate) fn new() -> Self {
        Self {
            transform: Affine::FLIP_Y, // svg is Y-down, fonts are Y-up
            path: Default::default(),
        }
    }

    fn to_svg_units(&self, x: f32, y: f32) -> Point {
        self.transform * Point::new(x as f64, y as f64)
    }

    pub(crate) fn to_svg_path(&self) -> String {
        // We use this rather than [`BezPath::to_svg`]` so we can exactly match the output of the tool we seek to replace
        let mut svg = String::new();
        let mut subpath_start = Point::default();
        let mut curr = Point::default();
        for el in self.path.elements() {
            match el {
                PathEl::MoveTo(p) => {
                    push_point(&mut svg, 'M', *p);
                    subpath_start = *p;
                    curr = *p;
                }
                PathEl::LineTo(p) => {
                    push_point(&mut svg, 'L', *p);
                    curr = *p;
                }
                PathEl::QuadTo(p1, p2) => {
                    push_point(&mut svg, 'Q', *p1);
                    push_point(&mut svg, ' ', *p2);
                    curr = *p2;
                }
                PathEl::CurveTo(p1, p2, p3) => {
                    push_point(&mut svg, 'C', *p1);
                    push_point(&mut svg, ' ', *p2);
                    push_point(&mut svg, ' ', *p3);
                    curr = *p3;
                }
                PathEl::ClosePath => {
                    // See <https://github.com/harfbuzz/harfbuzz/blob/2da79f70a1d562d883bdde5b74f6603374fb7023/src/hb-draw.hh#L148-L150>
                    if curr != subpath_start {
                        push_point(&mut svg, 'L', subpath_start);
                    }
                    svg.push('Z')
                }
            }
        }
        svg
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
