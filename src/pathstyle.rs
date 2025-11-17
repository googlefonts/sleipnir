//! Controls how a [`BezPath`] is converted to string form.
use crate::draw_commands::{DrawingCommand, DrawingCommandType};
use kurbo::{BezPath, PathEl, Point};

#[derive(Debug, Copy, Clone)]
pub enum SvgPathStyle {
    /// Emit the exact drawing commands received by the pen.
    ///
    /// This makes sense when you want to retain interpolation compatibility or to
    /// do your own post-processing later.
    Unchanged(usize),
    /// Try to produce a compact path
    ///
    /// Apply the optimizations from [Svgo convertPathData.js](https://github.com/Svg/Svgo/blob/main/plugins/convertPathData.js)
    /// that seem to have the greatest benefit for our use cases.
    Compact(usize),
}

impl SvgPathStyle {
    fn precision(&self) -> usize {
        match self {
            Self::Unchanged(p) | Self::Compact(p) => *p,
        }
    }

    /// Compare two values after rounding to precision
    fn round_eq<T: Rounding + PartialEq + Copy>(&self, p1: T, p2: T) -> bool {
        p1.round_prec(self.precision()) == p2.round_prec(self.precision())
    }

    pub(crate) fn write_svg_path(&self, path: &BezPath) -> String {
        self.write_path(path, DrawingCommandType::Svg)
    }

    pub(crate) fn write_kt_path(&self, path: &BezPath) -> String {
        self.write_path(path, DrawingCommandType::Kt)
    }

    fn write_path(&self, path: &BezPath, draw_type: DrawingCommandType) -> String {
        match self {
            SvgPathStyle::Unchanged(_) => to_unchanged_path(path, draw_type, *self),
            SvgPathStyle::Compact(_) => to_compact_path(path, draw_type, *self),
        }
    }
}

trait Rounding {
    fn round_prec(self, precision: usize) -> Self;
}

impl Rounding for f64 {
    fn round_prec(self, precision: usize) -> Self {
        let multiplier = 10f64.powi(precision as i32);
        (self * multiplier).round() / multiplier
    }
}

impl Rounding for Point {
    fn round_prec(self, precision: usize) -> Self {
        Point {
            x: self.x.round_prec(precision),
            y: self.y.round_prec(precision),
        }
    }
}

trait ToSvgCoord {
    fn write_absolute_coord(
        &self,
        path_style: SvgPathStyle,
        draw_type: DrawingCommandType,
    ) -> String;
    fn write_relative_coord(
        &self,
        other: Self,
        path_style: SvgPathStyle,
        draw_type: DrawingCommandType,
    ) -> String;
}

impl ToSvgCoord for f64 {
    fn write_absolute_coord(
        &self,
        path_style: SvgPathStyle,
        draw_type: DrawingCommandType,
    ) -> String {
        let mut val = round_coord(*self, path_style.precision());
        if draw_type == DrawingCommandType::Kt {
            val.push('f');
        }
        val
    }

    fn write_relative_coord(
        &self,
        other: Self,
        path_style: SvgPathStyle,
        draw_type: DrawingCommandType,
    ) -> String {
        (self - other).write_absolute_coord(path_style, draw_type)
    }
}

impl ToSvgCoord for Point {
    fn write_absolute_coord(
        &self,
        path_style: SvgPathStyle,
        draw_type: DrawingCommandType,
    ) -> String {
        let x_str = self.x.write_absolute_coord(path_style, draw_type);
        let y_str = self.y.write_absolute_coord(path_style, draw_type);

        match draw_type {
            DrawingCommandType::Kt => format!("{}, {}", x_str, y_str),
            DrawingCommandType::Svg => {
                if self.y < 0.0 {
                    format!("{}{}", x_str, y_str)
                } else {
                    format!("{},{}", x_str, y_str)
                }
            }
        }
    }

    fn write_relative_coord(
        &self,
        other: Self,
        path_style: SvgPathStyle,
        draw_type: DrawingCommandType,
    ) -> String {
        (*self - other)
            .to_point()
            .write_absolute_coord(path_style, draw_type)
    }
}

fn add_command<T, const N: usize>(
    svg: &mut String,
    path_style: SvgPathStyle,
    draw_type: DrawingCommandType,
    command: DrawingCommand,
    coords: [T; N],
    relative_to: Option<T>,
) where
    T: ToSvgCoord + Copy,
{
    let absolute = draw_type.collect_coords(
        coords
            .iter()
            .map(|p| p.write_absolute_coord(path_style, draw_type)),
    );

    let relative = relative_to.map(|rel_to| {
        draw_type.collect_coords(
            coords
                .iter()
                .map(|p| p.write_relative_coord(rel_to, path_style, draw_type)),
        )
    });

    svg.push_str(draw_type.padding());
    if relative.as_ref().map(|s| s.len()).unwrap_or(usize::MAX) < absolute.len() {
        svg.push_str(command.rel);
        svg.push_str(&relative.unwrap());
    } else {
        svg.push_str(command.abs);
        svg.push_str(&absolute);
    }
}

fn to_unchanged_path(
    path: &BezPath,
    draw_type: DrawingCommandType,
    path_style: SvgPathStyle,
) -> String {
    let mut svg = String::new();
    let mut subpath_start = Point::default();
    let mut curr = Point::default();
    for el in path.elements() {
        match el {
            PathEl::MoveTo(p) => {
                add_command(
                    &mut svg,
                    path_style,
                    draw_type,
                    draw_type.move_cmd(),
                    [*p],
                    None,
                );
                subpath_start = *p;
                curr = *p;
            }
            PathEl::LineTo(p) => {
                add_command(
                    &mut svg,
                    path_style,
                    draw_type,
                    draw_type.line_cmd(),
                    [*p],
                    None,
                );
                curr = *p;
            }
            PathEl::QuadTo(p1, p2) => {
                add_command(
                    &mut svg,
                    path_style,
                    draw_type,
                    draw_type.quad_cmd(),
                    [*p1, *p2],
                    None,
                );
                curr = *p2;
            }
            PathEl::CurveTo(p1, p2, p3) => {
                add_command(
                    &mut svg,
                    path_style,
                    draw_type,
                    draw_type.curve_cmd(),
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
                        path_style,
                        draw_type,
                        draw_type.line_cmd(),
                        [subpath_start],
                        None,
                    );
                }
                svg.push_str(draw_type.close_cmd().abs);
                curr = subpath_start;
            }
        }
    }
    svg
}

fn compact_line_to(
    svg: &mut String,
    draw_type: DrawingCommandType,
    path_style: SvgPathStyle,
    p: Point,
    curr: Point,
) {
    if p.x == curr.x {
        add_command(
            svg,
            path_style,
            draw_type,
            draw_type.vertical_line_cmd(),
            [p.y],
            Some(curr.y),
        );
    } else if p.y == curr.y {
        add_command(
            svg,
            path_style,
            draw_type,
            draw_type.horizontal_line_cmd(),
            [p.x],
            Some(curr.x),
        );
    } else {
        add_command(
            svg,
            path_style,
            draw_type,
            draw_type.line_cmd(),
            [p],
            Some(curr),
        );
    }
}

fn implied_control(prior_control: Point, prior_end: Point) -> Point {
    // The implied control is the reflection of the prior control over the prior end
    prior_control + 2.0 * (prior_end - prior_control)
}

fn try_add_smooth_quad(
    svg: &mut String,
    draw_type: DrawingCommandType,
    path_style: SvgPathStyle,
    prev: Option<PathEl>,
    p1: Point,
    p2: Point,
) -> bool {
    let Some(PathEl::QuadTo(prev_p1, prev_p2)) = prev else {
        return false;
    };

    if path_style.round_eq(implied_control(prev_p1, prev_p2), p1) {
        add_command(
            svg,
            path_style,
            draw_type,
            draw_type.smooth_quad_cmd(),
            [p2],
            Some(prev_p2),
        );
        true
    } else {
        false
    }
}

fn try_add_smooth_curve(
    svg: &mut String,
    draw_type: DrawingCommandType,
    path_style: SvgPathStyle,
    prev: Option<PathEl>,
    p1: Point,
    p2: Point,
    p3: Point,
) -> bool {
    let Some(PathEl::CurveTo(_, prev_p2, prev_p3)) = prev else {
        return false;
    };

    if path_style.round_eq(implied_control(prev_p2, prev_p3), p1) {
        add_command(
            svg,
            path_style,
            draw_type,
            draw_type.smooth_curve_cmd(),
            [p2, p3],
            Some(prev_p3),
        );
        true
    } else {
        false
    }
}

fn to_compact_path(
    path: &BezPath,
    draw_type: DrawingCommandType,
    path_style: SvgPathStyle,
) -> String {
    let mut svg = String::new();
    let mut subpath_start = Point::default();
    let mut curr = Point::default();
    let mut prev = None;
    for el in path.elements() {
        match el {
            PathEl::MoveTo(p) => {
                add_command(
                    &mut svg,
                    path_style,
                    draw_type,
                    draw_type.move_cmd(),
                    [*p],
                    Some(curr),
                );
                subpath_start = *p;
                curr = *p;
            }
            PathEl::LineTo(p) => {
                if !path_style.round_eq(curr, *p) {
                    compact_line_to(&mut svg, draw_type, path_style, *p, curr);
                }
                curr = *p;
            }
            PathEl::QuadTo(p1, p2) => {
                if !path_style.round_eq(curr, *p2)
                    && !try_add_smooth_quad(&mut svg, draw_type, path_style, prev, *p1, *p2)
                {
                    add_command(
                        &mut svg,
                        path_style,
                        draw_type,
                        draw_type.quad_cmd(),
                        [*p1, *p2],
                        Some(curr),
                    );
                }
                curr = *p2;
            }
            PathEl::CurveTo(p1, p2, p3) => {
                if !path_style.round_eq(curr, *p3)
                    && !try_add_smooth_curve(&mut svg, draw_type, path_style, prev, *p1, *p2, *p3)
                {
                    add_command(
                        &mut svg,
                        path_style,
                        draw_type,
                        draw_type.curve_cmd(),
                        [*p1, *p2, *p3],
                        Some(curr),
                    );
                }
                curr = *p3;
            }
            PathEl::ClosePath => {
                // See <https://github.com/harfbuzz/harfbuzz/blob/2da79f70a1d562d883bdde5b74f6603374fb7023/src/hb-draw.hh#L148-L150>
                if !path_style.round_eq(curr, subpath_start) {
                    compact_line_to(&mut svg, draw_type, path_style, subpath_start, curr);
                }
                svg.push_str(draw_type.close_cmd().abs);
                curr = subpath_start;
            }
        }
        prev = Some(*el);
    }
    svg
}

fn round_coord(pt: f64, precision: usize) -> String {
    let mut s = format!("{}", pt.round_prec(precision));

    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use kurbo::{BezPath, Point};

    use crate::pathstyle::{DrawingCommandType, SvgPathStyle, ToSvgCoord};

    #[test]
    fn coord_string_svg() {
        assert_eq!(
            vec!["2,3", "1-1", "2,3", "1-1"],
            vec![
                Point::new(2.0, 3.0)
                    .write_absolute_coord(SvgPathStyle::Compact(2), DrawingCommandType::Svg),
                Point::new(1.0, -1.0)
                    .write_absolute_coord(SvgPathStyle::Compact(2), DrawingCommandType::Svg),
                Point::new(2.0, 3.0)
                    .write_absolute_coord(SvgPathStyle::Unchanged(2), DrawingCommandType::Svg),
                Point::new(1.0, -1.0)
                    .write_absolute_coord(SvgPathStyle::Unchanged(2), DrawingCommandType::Svg),
            ],
        );
    }

    #[test]
    fn coord_string_kt() {
        assert_eq!(
            vec!["2f, 3f", "-1f, -1f", "2f, 3f", "-1f, -1.57f"],
            vec![
                Point::new(2.0, 3.0)
                    .write_absolute_coord(SvgPathStyle::Compact(2), DrawingCommandType::Kt),
                Point::new(-1.0, -1.0)
                    .write_absolute_coord(SvgPathStyle::Compact(2), DrawingCommandType::Kt),
                Point::new(2.0, 3.0)
                    .write_absolute_coord(SvgPathStyle::Unchanged(2), DrawingCommandType::Kt),
                Point::new(-1.0, -1.5677777)
                    .write_absolute_coord(SvgPathStyle::Unchanged(2), DrawingCommandType::Kt),
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
            SvgPathStyle::Unchanged(2).write_svg_path(&path),
            "M1,1L2,2L3,2L3,3L1.25,3L1.25,1.5L1,1Z"
        );
        assert_eq!(
            SvgPathStyle::Compact(2).write_svg_path(&path),
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
            SvgPathStyle::Unchanged(2).write_svg_path(&path),
            "M10,10L11,11Q15,19 20,20L19,20L19,19C23,17 12,14 10,11L10,10Z"
        );
        assert_eq!(
            SvgPathStyle::Compact(2).write_svg_path(&path),
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
            SvgPathStyle::Unchanged(2).write_svg_path(&path),
            "M-10-10L-5-5L-11-5C-15-7-8-8-10-10Z"
        );
        assert_eq!(
            SvgPathStyle::Compact(2).write_svg_path(&path),
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
            SvgPathStyle::Unchanged(2).write_svg_path(&path),
            "M1,1L1,1Q2,1 2,2Q8,10 2,2C33,2-5,3 0,3C33,2-5,3 0,3L1,1Z"
        );
        // Note that the pointless (pen doesn't move after rounding) commands are dropped
        assert_eq!(
            SvgPathStyle::Compact(2).write_svg_path(&path),
            "M1,1Q2,1 2,2C33,2-5,3 0,3L1,1Z"
        );
    }

    #[test]
    fn prefer_smooth_quad() {
        // from a real icon Svg
        // M160-160Q127-160 103.5-183.5Q80-207 80-240
        let mut path = BezPath::new();
        path.move_to((160.0, -160.0));
        path.quad_to((127.0, -160.0), (103.5, -183.5));
        path.quad_to((80.0, -207.0), (80.0, -240.0));
        path.close_path();

        assert_eq!(
            SvgPathStyle::Unchanged(2).write_svg_path(&path),
            "M160-160Q127-160 103.5-183.5Q80-207 80-240L160-160Z"
        );
        assert_eq!(
            SvgPathStyle::Compact(2).write_svg_path(&path),
            "M160-160q-33,0-56.5-23.5T80-240l80,80Z"
        );
    }

    #[test]
    fn prefer_smooth_cubic() {
        // Derived from example at https://developer.mozilla.org/en-US/docs/Web/Svg/Attribute/d#path_commands
        let mut path = BezPath::new();
        path.move_to((10.0, 90.0));
        path.curve_to((30.0, 90.0), (25.0, 10.0), (50.0, 10.0));
        path.curve_to((75.0, 10.0), (70.0, 90.0), (90.0, 90.0));

        assert_eq!(
            SvgPathStyle::Unchanged(2).write_svg_path(&path),
            "M10,90C30,90 25,10 50,10C75,10 70,90 90,90"
        );
        assert_eq!(
            SvgPathStyle::Compact(2).write_svg_path(&path),
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
            SvgPathStyle::Unchanged(2).write_svg_path(&path),
            "M10,20L15,15L5,15L10,20ZM10,25L15,30L5,30L10,25Z"
        );
        assert_eq!(
            SvgPathStyle::Compact(2).write_svg_path(&path),
            "M10,20l5-5H5l5,5Zm0,5l5,5H5l5-5Z"
        );
    }

    #[test]
    fn test_round_coord() {
        assert_eq!("1", super::round_coord(1.001, 2));
        assert_eq!("1.2", super::round_coord(1.203, 2));
        assert_eq!("1.21", super::round_coord(1.205, 2));
        assert_eq!("1.21", super::round_coord(1.207, 2));
    }
}
