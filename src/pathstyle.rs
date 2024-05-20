//! Controls how a [`BezPath`] is converted to string form.

use kurbo::{BezPath, PathEl, Point};

#[derive(Debug, Copy, Clone)]
pub enum PathStyle {
    /// Emit the exact drawing commands received by the pen.
    ///
    /// This makes sense when you want to retain interpolation compatibility or to
    /// do your own post-processing later.
    Unchanged,
    /// Try to produce a compact path
    ///
    /// Apply the optimizations from [svgo convertPathData.js](https://github.com/svg/svgo/blob/main/plugins/convertPathData.js)
    /// that seem to have the greatest benefit for our use cases.
    Compact,
}

impl PathStyle {
    pub(crate) fn write_svg_path(&self, path: &BezPath) -> String {
        match self {
            PathStyle::Unchanged => to_unoptimized_svg_path(path),
            PathStyle::Compact => to_compact_svg_path(path),
        }
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

trait ToSvgCoord {
    fn write_absolute_coord(&self) -> String;
    fn write_relative_coord(&self, other: Self) -> String;
}

impl ToSvgCoord for f64 {
    fn write_absolute_coord(&self) -> String {
        format!("{}", round2(*self))
    }

    fn write_relative_coord(&self, other: Self) -> String {
        format!("{}", round2(*self - other))
    }
}

impl ToSvgCoord for Point {
    fn write_absolute_coord(&self) -> String {
        coord_string(*self)
    }

    fn write_relative_coord(&self, other: Self) -> String {
        coord_string((*self - other).to_point())
    }
}

fn coord_string(p: Point) -> String {
    format!("{},{}", round2(p.x), round2(p.y))
}

fn add_command<T, const N: usize>(
    svg: &mut String,
    prefix: char,
    coords: [T; N],
    relative_to: Option<T>,
) where
    T: ToSvgCoord + Copy,
{
    assert!(prefix.is_ascii_uppercase());

    let absolute = coords
        .iter()
        .map(|p| p.write_absolute_coord())
        .collect::<Vec<_>>()
        .join(" ");
    let relative = relative_to.map(|rel_to| {
        coords
            .iter()
            .map(|p| p.write_relative_coord(rel_to))
            .collect::<Vec<_>>()
            .join(" ")
    });

    if relative.as_ref().map(|s| s.len()).unwrap_or(usize::MAX) < absolute.len() {
        svg.push(prefix.to_ascii_lowercase());
        svg.push_str(&relative.unwrap());
    } else {
        svg.push(prefix);
        svg.push_str(&absolute);
    }
}

fn to_unoptimized_svg_path(path: &BezPath) -> String {
    let mut svg = String::new();
    let mut subpath_start = Point::default();
    let mut curr = Point::default();
    for el in path.elements() {
        match el {
            PathEl::MoveTo(p) => {
                add_command(&mut svg, 'M', [*p], None);
                subpath_start = *p;
                curr = *p;
            }
            PathEl::LineTo(p) => {
                add_command(&mut svg, 'L', [*p], None);
                curr = *p;
            }
            PathEl::QuadTo(p1, p2) => {
                add_command(&mut svg, 'Q', [*p1, *p2], None);
                curr = *p2;
            }
            PathEl::CurveTo(p1, p2, p3) => {
                add_command(&mut svg, 'C', [*p1, *p2, *p3], None);
                curr = *p3;
            }
            PathEl::ClosePath => {
                // See <https://github.com/harfbuzz/harfbuzz/blob/2da79f70a1d562d883bdde5b74f6603374fb7023/src/hb-draw.hh#L148-L150>
                if curr != subpath_start {
                    add_command(&mut svg, 'L', [subpath_start], None);
                }
                svg.push('Z')
            }
        }
    }
    svg
}

fn compact_line_to(svg: &mut String, p: Point, curr: Point) {
    if p.x == curr.x {
        add_command(svg, 'V', [p.y], Some(curr.y));
    } else if p.y == curr.y {
        add_command(svg, 'H', [p.x], Some(curr.x));
    } else {
        add_command(svg, 'L', [p], Some(curr));
    }
}

fn to_compact_svg_path(path: &BezPath) -> String {
    let mut svg = String::new();
    let mut subpath_start = Point::default();
    let mut curr = Point::default();
    for el in path.elements() {
        match el {
            PathEl::MoveTo(p) => {
                add_command(&mut svg, 'M', [*p], Some(curr));
                subpath_start = *p;
                curr = *p;
            }
            PathEl::LineTo(p) => {
                compact_line_to(&mut svg, *p, curr);
                curr = *p;
            }
            PathEl::QuadTo(p1, p2) => {
                add_command(&mut svg, 'Q', [*p1, *p2], Some(curr));
                curr = *p2;
            }
            PathEl::CurveTo(p1, p2, p3) => {
                add_command(&mut svg, 'C', [*p1, *p2, *p3], Some(curr));
                curr = *p3;
            }
            PathEl::ClosePath => {
                // See <https://github.com/harfbuzz/harfbuzz/blob/2da79f70a1d562d883bdde5b74f6603374fb7023/src/hb-draw.hh#L148-L150>
                if curr != subpath_start {
                    compact_line_to(&mut svg, subpath_start, curr);
                }
                svg.push('Z')
            }
        }
    }
    svg
}

#[cfg(test)]
mod tests {
    use kurbo::BezPath;

    use crate::pathstyle::PathStyle;

    #[test]
    fn compact_1d_lines() {
        let mut path = BezPath::new();
        path.move_to((1.0, 1.0));
        path.line_to((2.0, 2.0));
        path.line_to((3.0, 2.0));
        path.line_to((3.0, 3.0));
        path.line_to((1.25, 3.0));
        path.line_to((1.25, 1.5));
        path.close_path();

        assert_eq!(
            PathStyle::Unchanged.write_svg_path(&path),
            "M1,1L2,2L3,2L3,3L1.25,3L1.25,1.5L1,1Z"
        );
        assert_eq!(
            PathStyle::Compact.write_svg_path(&path),
            "M1,1L2,2H3V3H1.25V1.5L1,1Z"
        );
    }

    #[test]
    fn relative_when_shorter() {
        let mut path = BezPath::new();
        path.move_to((10.0, 10.0));
        path.line_to((11.0, 11.0));
        path.quad_to((15.0, 19.0), (20.0, 20.0));
        path.line_to((19.0, 20.0));
        path.line_to((19.0, 19.0));
        path.curve_to((23.0, 17.0), (12.0, 14.0), (10.0, 11.0));
        path.close_path();

        assert_eq!(
            PathStyle::Unchanged.write_svg_path(&path),
            "M10,10L11,11Q15,19 20,20L19,20L19,19C23,17 12,14 10,11L10,10Z"
        );
        assert_eq!(
            PathStyle::Compact.write_svg_path(&path),
            "M10,10l1,1q4,8 9,9H19V19c4,-2 -7,-5 -9,-8V10Z"
        );
    }
}
