//! Controls how a [`BezPath`] is converted to string form.

use kurbo::{BezPath, PathEl, Point};

#[derive(Debug, Copy, Clone)]
pub enum SvgPathStyle {
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

impl SvgPathStyle {
    pub(crate) fn write_svg_path(&self, path: &BezPath) -> String {
        match self {
            SvgPathStyle::Unchanged => to_unchanged_svg_path(path),
            SvgPathStyle::Compact => to_compact_svg_path(path),
        }
    }

    fn coord_string(self, p: Point) -> String {
        let p = p.round2();
        if matches!(self, SvgPathStyle::Compact) && p.y < 0.0 {
            format!("{}{}", p.x, p.y)
        } else {
            format!("{},{}", p.x, p.y)
        }
    }
}

trait Round2 {
    fn round2(self) -> Self;
}

impl Round2 for f64 {
    fn round2(self) -> Self {
        (self * 100.0).round() / 100.0
    }
}

impl Round2 for Point {
    fn round2(self) -> Self {
        Point {
            x: self.x.round2(),
            y: self.y.round2(),
        }
    }
}

trait ToSvgCoord {
    fn write_absolute_coord(&self, path_style: SvgPathStyle) -> String;
    fn write_relative_coord(&self, other: Self, path_style: SvgPathStyle) -> String;
}

impl ToSvgCoord for f64 {
    fn write_absolute_coord(&self, _: SvgPathStyle) -> String {
        format!("{}", self.round2())
    }

    fn write_relative_coord(&self, other: Self, _: SvgPathStyle) -> String {
        format!("{}", (self - other).round2())
    }
}

impl ToSvgCoord for Point {
    fn write_absolute_coord(&self, path_style: SvgPathStyle) -> String {
        path_style.coord_string(*self)
    }

    fn write_relative_coord(&self, other: Self, path_style: SvgPathStyle) -> String {
        path_style.coord_string((*self - other).to_point())
    }
}

/// Transient type used to enable collection of multiple coordinate strings to a compact path string
///
/// In particular, we _sometimes_ add a joining character. This type gives us something to hang the
/// FromIterator implementation from.
struct SvgCoords(String);

impl FromIterator<String> for SvgCoords {
    fn from_iter<T: IntoIterator<Item = String>>(iter: T) -> Self {
        let mut path = String::with_capacity(256);
        for coord in iter.into_iter() {
            // No space required?
            if !path.is_empty() && !coord.starts_with('-') {
                path.push(' ');
            }
            path.push_str(&coord);
        }
        SvgCoords(path)
    }
}

fn add_command<T, const N: usize>(
    svg: &mut String,
    path_style: SvgPathStyle,
    prefix: char,
    coords: [T; N],
    relative_to: Option<T>,
) where
    T: ToSvgCoord + Copy,
{
    assert!(prefix.is_ascii_uppercase());

    let absolute = coords
        .iter()
        .map(|p| p.write_absolute_coord(path_style))
        .collect::<SvgCoords>()
        .0;
    let relative = relative_to.map(|rel_to| {
        coords
            .iter()
            .map(|p| p.write_relative_coord(rel_to, path_style))
            .collect::<SvgCoords>()
            .0
    });

    if relative.as_ref().map(|s| s.len()).unwrap_or(usize::MAX) < absolute.len() {
        svg.push(prefix.to_ascii_lowercase());
        svg.push_str(&relative.unwrap());
    } else {
        svg.push(prefix);
        svg.push_str(&absolute);
    }
}

fn to_unchanged_svg_path(path: &BezPath) -> String {
    let mut svg = String::new();
    let mut subpath_start = Point::default();
    let mut curr = Point::default();
    for el in path.elements() {
        match el {
            PathEl::MoveTo(p) => {
                add_command(&mut svg, SvgPathStyle::Unchanged, 'M', [*p], None);
                subpath_start = *p;
                curr = *p;
            }
            PathEl::LineTo(p) => {
                add_command(&mut svg, SvgPathStyle::Unchanged, 'L', [*p], None);
                curr = *p;
            }
            PathEl::QuadTo(p1, p2) => {
                add_command(&mut svg, SvgPathStyle::Unchanged, 'Q', [*p1, *p2], None);
                curr = *p2;
            }
            PathEl::CurveTo(p1, p2, p3) => {
                add_command(
                    &mut svg,
                    SvgPathStyle::Unchanged,
                    'C',
                    [*p1, *p2, *p3],
                    None,
                );
                curr = *p3;
            }
            PathEl::ClosePath => {
                // See <https://github.com/harfbuzz/harfbuzz/blob/2da79f70a1d562d883bdde5b74f6603374fb7023/src/hb-draw.hh#L148-L150>
                if curr != subpath_start {
                    add_command(
                        &mut svg,
                        SvgPathStyle::Unchanged,
                        'L',
                        [subpath_start],
                        None,
                    );
                }
                svg.push('Z');
                curr = subpath_start;
            }
        }
    }
    svg
}

fn compact_line_to(svg: &mut String, p: Point, curr: Point) {
    if p.x == curr.x {
        add_command(svg, SvgPathStyle::Compact, 'V', [p.y], Some(curr.y));
    } else if p.y == curr.y {
        add_command(svg, SvgPathStyle::Compact, 'H', [p.x], Some(curr.x));
    } else {
        add_command(svg, SvgPathStyle::Compact, 'L', [p], Some(curr));
    }
}

fn implied_control(prior_control: Point, prior_end: Point) -> Point {
    // The implied control is the reflection of the prior control over the prior end
    prior_control + 2.0 * (prior_end - prior_control)
}

fn try_add_smooth_quad(svg: &mut String, prev: Option<PathEl>, p1: Point, p2: Point) -> bool {
    let Some(PathEl::QuadTo(prev_p1, prev_p2)) = prev else {
        return false;
    };

    if implied_control(prev_p1, prev_p2).round2() == p1.round2() {
        add_command(svg, SvgPathStyle::Compact, 'T', [p2], Some(prev_p2));
        true
    } else {
        false
    }
}

fn try_add_smooth_curve(
    svg: &mut String,
    prev: Option<PathEl>,
    p1: Point,
    p2: Point,
    p3: Point,
) -> bool {
    let Some(PathEl::CurveTo(_, prev_p2, prev_p3)) = prev else {
        return false;
    };

    if implied_control(prev_p2, prev_p3).round2() == p1.round2() {
        add_command(svg, SvgPathStyle::Compact, 'S', [p2, p3], Some(prev_p3));
        true
    } else {
        false
    }
}

fn to_compact_svg_path(path: &BezPath) -> String {
    let mut svg = String::new();
    let mut subpath_start = Point::default();
    let mut curr = Point::default();
    let mut prev = None;
    for el in path.elements() {
        match el {
            PathEl::MoveTo(p) => {
                add_command(&mut svg, SvgPathStyle::Compact, 'M', [*p], Some(curr));
                subpath_start = *p;
                curr = *p;
            }
            PathEl::LineTo(p) => {
                if curr.round2() != p.round2() {
                    compact_line_to(&mut svg, *p, curr);
                }
                curr = *p;
            }
            PathEl::QuadTo(p1, p2) => {
                if curr.round2() != p2.round2() && !try_add_smooth_quad(&mut svg, prev, *p1, *p2) {
                    add_command(&mut svg, SvgPathStyle::Compact, 'Q', [*p1, *p2], Some(curr));
                }
                curr = *p2;
            }
            PathEl::CurveTo(p1, p2, p3) => {
                if curr.round2() != p3.round2()
                    && !try_add_smooth_curve(&mut svg, prev, *p1, *p2, *p3)
                {
                    add_command(
                        &mut svg,
                        SvgPathStyle::Compact,
                        'C',
                        [*p1, *p2, *p3],
                        Some(curr),
                    );
                }
                curr = *p3;
            }
            PathEl::ClosePath => {
                // See <https://github.com/harfbuzz/harfbuzz/blob/2da79f70a1d562d883bdde5b74f6603374fb7023/src/hb-draw.hh#L148-L150>
                if curr.round2() != subpath_start.round2() {
                    compact_line_to(&mut svg, subpath_start, curr);
                }
                svg.push('Z');
                curr = subpath_start;
            }
        }
        prev = Some(*el);
    }
    svg
}

#[cfg(test)]
mod tests {
    use kurbo::BezPath;

    use crate::pathstyle::SvgPathStyle;

    #[test]
    fn coord_string() {
        assert_eq!(
            vec!["2,3", "1-1", "2,3", "1,-1"],
            vec![
                SvgPathStyle::Compact.coord_string((2.0, 3.0).into()),
                SvgPathStyle::Compact.coord_string((1.0, -1.0).into()),
                SvgPathStyle::Unchanged.coord_string((2.0, 3.0).into()),
                SvgPathStyle::Unchanged.coord_string((1.0, -1.0).into()),
            ],
        );
    }

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
            SvgPathStyle::Unchanged.write_svg_path(&path),
            "M1,1L2,2L3,2L3,3L1.25,3L1.25,1.5L1,1Z"
        );
        assert_eq!(
            SvgPathStyle::Compact.write_svg_path(&path),
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
            SvgPathStyle::Unchanged.write_svg_path(&path),
            "M10,10L11,11Q15,19 20,20L19,20L19,19C23,17 12,14 10,11L10,10Z"
        );
        assert_eq!(
            SvgPathStyle::Compact.write_svg_path(&path),
            "M10,10l1,1q4,8 9,9H19V19c4-2-7-5-9-8V10Z"
        );
    }

    #[test]
    fn unspaced_negatives() {
        let mut path = BezPath::new();
        path.move_to((-10.0, -10.0));
        path.line_to((-5.0, -5.0));
        path.line_to((-11.0, -5.0));
        path.curve_to((-15.0, -7.0), (-8.0, -8.0), (-10.0, -10.0));
        path.close_path();

        assert_eq!(
            SvgPathStyle::Unchanged.write_svg_path(&path),
            "M-10,-10L-5,-5L-11,-5C-15,-7-8,-8-10,-10Z"
        );
        assert_eq!(
            SvgPathStyle::Compact.write_svg_path(&path),
            "M-10-10l5,5h-6c-4-2 3-3 1-5Z"
        );
    }

    #[test]
    fn remove_nop_commands() {
        let mut path = BezPath::new();
        path.move_to((1.0, 1.0));
        path.line_to((1.0, 1.0001)); // pointless after round2
        path.quad_to((2.0, 1.0), (2.0, 2.0));
        path.quad_to((8.0, 10.0), (2.001, 1.99999)); // pointless after round2
        path.curve_to((33.0, 2.0), (-5.0, 3.0), (0.0, 3.0));
        path.curve_to((33.0, 2.0), (-5.0, 3.0), (0.001, 3.0)); // pointless after round2
        path.close_path();

        assert_eq!(
            SvgPathStyle::Unchanged.write_svg_path(&path),
            "M1,1L1,1Q2,1 2,2Q8,10 2,2C33,2-5,3 0,3C33,2-5,3 0,3L1,1Z"
        );
        // Note that the pointless (pen doesn't move after rounding) commands are dropped
        assert_eq!(
            SvgPathStyle::Compact.write_svg_path(&path),
            "M1,1Q2,1 2,2C33,2-5,3 0,3L1,1Z"
        );
    }

    #[test]
    fn prefer_smooth_quad() {
        // from a real icon svg
        // M160,-160Q127,-160 103.5,-183.5Q80,-207 80,-240
        let mut path = BezPath::new();
        path.move_to((160.0, -160.0));
        path.quad_to((127.0, -160.0), (103.5, -183.5));
        path.quad_to((80.0, -207.0), (80.0, -240.0));
        path.close_path();

        assert_eq!(
            SvgPathStyle::Unchanged.write_svg_path(&path),
            "M160,-160Q127,-160 103.5,-183.5Q80,-207 80,-240L160,-160Z"
        );
        assert_eq!(
            SvgPathStyle::Compact.write_svg_path(&path),
            "M160-160q-33,0-56.5-23.5T80-240l80,80Z"
        );
    }

    #[test]
    fn prefer_smooth_cubic() {
        // Derived from example at https://developer.mozilla.org/en-US/docs/Web/SVG/Attribute/d#path_commands
        let mut path = BezPath::new();
        path.move_to((10.0, 90.0));
        path.curve_to((30.0, 90.0), (25.0, 10.0), (50.0, 10.0));
        path.curve_to((75.0, 10.0), (70.0, 90.0), (90.0, 90.0));

        assert_eq!(
            SvgPathStyle::Unchanged.write_svg_path(&path),
            "M10,90C30,90 25,10 50,10C75,10 70,90 90,90"
        );
        assert_eq!(
            SvgPathStyle::Compact.write_svg_path(&path),
            "M10,90c20,0 15-80 40-80S70,90 90,90"
        );
    }

    // They once didn't and terrible things would happen to multi-subpath paths
    #[test]
    fn close_path_updates_current() {
        let mut path = BezPath::new();
        path.move_to((10.0, 20.0));
        path.line_to((15.0, 15.0));
        path.line_to((5.0, 15.0));
        path.close_path();
        // Relative move will be shorter, m0,5 vs M10,25 ... if close updated current
        path.move_to((10.0, 25.0));
        path.line_to((15.0, 30.0));
        path.line_to((5.0, 30.0));
        path.close_path();

        assert_eq!(
            SvgPathStyle::Unchanged.write_svg_path(&path),
            "M10,20L15,15L5,15L10,20ZM10,25L15,30L5,30L10,25Z"
        );
        assert_eq!(
            SvgPathStyle::Compact.write_svg_path(&path),
            "M10,20l5-5H5l5,5Zm0,5l5,5H5l5-5Z"
        );
    }
}
