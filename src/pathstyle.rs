//! Controls how a [`BezPath`] is converted to string form.
mod minimized;

use crate::draw_commands::{DrawingCommand, DrawingCommandType};
use kurbo::{BezPath, PathEl, Point};

/// An iterator over path elements that tracks the current position.
pub struct PathWithPosIter<'a> {
    elements: std::slice::Iter<'a, PathEl>,
    curr: Point,
    subpath_start: Point,
}

impl<'a> PathWithPosIter<'a> {
    /// Create an iterator over `path` that yields each element with the pen position.
    pub fn new(path: &'a BezPath) -> Self {
        Self {
            elements: path.elements().iter(),
            curr: Point::default(),
            subpath_start: Point::default(),
        }
    }
}

/// A path element along with the current position before executing it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathElWithPos {
    /// The path element being processed.
    pub element: PathEl,
    /// Pen position before this element is applied.
    pub curr: Point,
    /// Start of the current subpath before this element is applied.
    pub subpath_start: Point,
}

impl<'a> Iterator for PathWithPosIter<'a> {
    type Item = PathElWithPos;

    fn next(&mut self) -> Option<Self::Item> {
        let el = *self.elements.next()?;
        let result = PathElWithPos {
            element: el,
            curr: self.curr,
            subpath_start: self.subpath_start,
        };

        // Update current position based on the element
        match el {
            PathEl::MoveTo(p) => {
                self.subpath_start = p;
                self.curr = p;
            }
            PathEl::LineTo(p) => {
                self.curr = p;
            }
            PathEl::QuadTo(_, p2) => {
                self.curr = p2;
            }
            PathEl::CurveTo(_, _, p3) => {
                self.curr = p3;
            }
            PathEl::ClosePath => {
                self.curr = self.subpath_start;
            }
        }

        Some(result)
    }
}

pub use minimized::MakeArcs;

/// Controls how a [`BezPath`] is converted to an SVG or Kotlin path string, with individual
/// optimization passes that can be toggled on and off.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct SvgPathStyle {
    /// Number of decimal places to round coordinates to (`SVGO floatPrecision`).
    pub precision: usize,
    /// Emit whichever of absolute or relative coordinates is shorter (`SVGO utilizeAbsolute`).
    pub utilize_absolute: bool,
    /// Convert horizontal and vertical lines to `H`/`h` and `V`/`v` (`SVGO lineShorthands`).
    pub line_shorthands: bool,
    /// Convert smooth quadratic/cubic curves to `T`/`t` and `S`/`s` (`SVGO curveSmoothShorthands`).
    pub curve_smooth_shorthands: bool,
    /// Drop zero-length segments and degenerate point-only subpaths (`SVGO removeUseless`).
    pub remove_useless: bool,
    /// Drop a redundant closing line before `Z` when it returns to the subpath start (`SVGO convertToZ`).
    pub convert_to_z: bool,
    /// Merge consecutive collinear line segments into a single line (`SVGO collapseRepeated`).
    pub collapse_collinear: bool,
    /// Omit repeated command letters and use implicit `L`/`l` after `M`/`m` (`SVGO stringifyPathData`).
    pub elide_command_letters: bool,
    /// Strip leading zeros from decimals in `(-1, 1)` (`0.5` -> `.5`) (`SVGO leadingZero`).
    pub leading_zero: bool,
    /// Convert circular quadratic Bézier runs into SVG `A`/`a` arcs (`SVGO makeArcs`).
    pub make_arcs: Option<MakeArcs>,
}

impl SvgPathStyle {
    /// Emit the exact drawing commands received by the pen, rounded to `precision` decimal places.
    ///
    /// This makes sense when you want to retain interpolation compatibility or to
    /// do your own post-processing later.
    pub const fn unchanged(precision: usize) -> Self {
        Self {
            precision,
            utilize_absolute: false,
            line_shorthands: false,
            curve_smooth_shorthands: false,
            remove_useless: false,
            convert_to_z: false,
            collapse_collinear: false,
            elide_command_letters: false,
            leading_zero: false,
            make_arcs: None,
        }
    }

    /// Try to produce a compact path.
    ///
    /// Applies the optimizations from [Svgo convertPathData.js](https://github.com/Svg/Svgo/blob/main/plugins/convertPathData.js)
    /// that have the greatest benefit for our use cases.
    pub const fn compact(precision: usize) -> Self {
        Self {
            precision,
            utilize_absolute: true,
            line_shorthands: true,
            curve_smooth_shorthands: true,
            remove_useless: true,
            convert_to_z: true,
            collapse_collinear: true,
            elide_command_letters: true,
            leading_zero: true,
            make_arcs: Some(MakeArcs::lossy()),
        }
    }

    /// Produce a compact path using only lossless syntax rewrites (no curve-to-arc fitting).
    pub const fn lossless_compact(precision: usize) -> Self {
        Self {
            make_arcs: None,
            ..Self::compact(precision)
        }
    }

    /// Backward-compatible constructor for [`SvgPathStyle::unchanged`].
    #[allow(non_snake_case)]
    pub const fn Unchanged(precision: usize) -> Self {
        Self::unchanged(precision)
    }

    /// Backward-compatible constructor for [`SvgPathStyle::compact`].
    #[allow(non_snake_case)]
    pub const fn Compact(precision: usize) -> Self {
        Self::compact(precision)
    }

    /// Set the decimal rounding precision.
    pub const fn with_precision(mut self, precision: usize) -> Self {
        self.precision = precision;
        self
    }

    /// Enable or disable choosing the shorter of absolute vs. relative coordinates.
    pub const fn with_utilize_absolute(mut self, enabled: bool) -> Self {
        self.utilize_absolute = enabled;
        self
    }

    /// Enable or disable `H`/`h` and `V`/`v` line shorthands.
    pub const fn with_line_shorthands(mut self, enabled: bool) -> Self {
        self.line_shorthands = enabled;
        self
    }

    /// Enable or disable `T`/`t` and `S`/`s` smooth curve shorthands.
    pub const fn with_curve_smooth_shorthands(mut self, enabled: bool) -> Self {
        self.curve_smooth_shorthands = enabled;
        self
    }

    /// Enable or disable dropping zero-length segments and point-only subpaths.
    pub const fn with_remove_useless(mut self, enabled: bool) -> Self {
        self.remove_useless = enabled;
        self
    }

    /// Enable or disable dropping redundant closing lines before `Z`.
    pub const fn with_convert_to_z(mut self, enabled: bool) -> Self {
        self.convert_to_z = enabled;
        self
    }

    /// Enable or disable merging consecutive collinear line segments.
    pub const fn with_collapse_collinear(mut self, enabled: bool) -> Self {
        self.collapse_collinear = enabled;
        self
    }

    /// Enable or disable omitting repeated command letters and implicit `L`/`l` after `M`/`m`.
    pub const fn with_elide_command_letters(mut self, enabled: bool) -> Self {
        self.elide_command_letters = enabled;
        self
    }

    /// Enable or disable stripping leading zeros from decimals in `(-1, 1)`.
    pub const fn with_leading_zero(mut self, enabled: bool) -> Self {
        self.leading_zero = enabled;
        self
    }

    /// Configure or disable converting circular quadratic Bézier runs into SVG `A`/`a` arcs.
    pub const fn with_make_arcs(mut self, make_arcs: Option<MakeArcs>) -> Self {
        self.make_arcs = make_arcs;
        self
    }

    /// Returns `true` if any optimization flag is enabled.
    pub const fn has_any_optimization(&self) -> bool {
        self.utilize_absolute
            || self.line_shorthands
            || self.curve_smooth_shorthands
            || self.remove_useless
            || self.convert_to_z
            || self.collapse_collinear
            || self.elide_command_letters
            || self.leading_zero
            || self.make_arcs.is_some()
    }

    fn precision(&self) -> usize {
        self.precision
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
        if !self.has_any_optimization() {
            return to_unchanged_path(path, draw_type, *self);
        }
        match draw_type {
            DrawingCommandType::Kt => to_compact_path(path, draw_type, *self),
            DrawingCommandType::Svg => minimized::to_minimized_svg_path(path, *self),
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

    for item in PathWithPosIter::new(path) {
        match item.element {
            PathEl::MoveTo(p) => {
                add_command(
                    &mut svg,
                    path_style,
                    draw_type,
                    draw_type.move_cmd(),
                    [p],
                    None,
                );
            }
            PathEl::LineTo(p) => {
                add_command(
                    &mut svg,
                    path_style,
                    draw_type,
                    draw_type.line_cmd(),
                    [p],
                    None,
                );
            }
            PathEl::QuadTo(p1, p2) => {
                add_command(
                    &mut svg,
                    path_style,
                    draw_type,
                    draw_type.quad_cmd(),
                    [p1, p2],
                    None,
                );
            }
            PathEl::CurveTo(p1, p2, p3) => {
                add_command(
                    &mut svg,
                    path_style,
                    draw_type,
                    draw_type.curve_cmd(),
                    [p1, p2, p3],
                    None,
                );
            }
            PathEl::ClosePath => {
                // See <https://github.com/harfbuzz/harfbuzz/blob/2da79f70a1d562d883bdde5b74f6603374fb7023/src/hb-draw.hh#L148-L150>
                if item.curr != item.subpath_start {
                    add_command(
                        &mut svg,
                        path_style,
                        draw_type,
                        draw_type.line_cmd(),
                        [item.subpath_start],
                        None,
                    );
                }
                svg.push_str(draw_type.close_cmd().abs);
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
    let mut prev = None;

    for item in PathWithPosIter::new(path) {
        let emitted = match item.element {
            PathEl::MoveTo(p) => {
                add_command(
                    &mut svg,
                    path_style,
                    draw_type,
                    draw_type.move_cmd(),
                    [p],
                    Some(item.curr),
                );
                true
            }
            PathEl::LineTo(p) => {
                let e = !path_style.round_eq(item.curr, p);
                if e {
                    compact_line_to(&mut svg, draw_type, path_style, p, item.curr);
                }
                e
            }
            PathEl::QuadTo(p1, p2) => {
                let e = !path_style.round_eq(item.curr, p2);
                if e && !try_add_smooth_quad(&mut svg, draw_type, path_style, prev, p1, p2) {
                    add_command(
                        &mut svg,
                        path_style,
                        draw_type,
                        draw_type.quad_cmd(),
                        [p1, p2],
                        Some(item.curr),
                    );
                }
                e
            }
            PathEl::CurveTo(p1, p2, p3) => {
                let e = !path_style.round_eq(item.curr, p3);
                if e && !try_add_smooth_curve(&mut svg, draw_type, path_style, prev, p1, p2, p3) {
                    add_command(
                        &mut svg,
                        path_style,
                        draw_type,
                        draw_type.curve_cmd(),
                        [p1, p2, p3],
                        Some(item.curr),
                    );
                }
                e
            }
            PathEl::ClosePath => {
                // See <https://github.com/harfbuzz/harfbuzz/blob/2da79f70a1d562d883bdde5b74f6603374fb7023/src/hb-draw.hh#L148-L150>
                if !path_style.round_eq(item.curr, item.subpath_start) {
                    compact_line_to(
                        &mut svg,
                        draw_type,
                        path_style,
                        item.subpath_start,
                        item.curr,
                    );
                }
                svg.push_str(draw_type.close_cmd().abs);
                true
            }
        };
        if emitted {
            prev = Some(item.element);
        }
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
            "M1,1l1,1h1v1H1.25V1.5Z"
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
            "M10,10l1,1q4,8 9,9h-1v-1c4-2-7-5-9-8Z"
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
            "M1,1q1,0 1,1c31,0-7,1-2,1Z"
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
            "M160-160a80,80 0 0,1-80-80Z"
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
            "M10,90c20,0 15-80 40-80s20,80 40,80"
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
            "M10,20l5-5H5Zm0,5 5,5H5Z"
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
