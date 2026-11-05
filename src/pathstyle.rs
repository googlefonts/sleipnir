//! Controls how a [`BezPath`] is converted to string form.
mod arcs;

use crate::draw_commands::{DrawingCommand, DrawingCommandType};
use arcs::CircularArc;
use kurbo::{BezPath, PathEl, Point, QuadBez};

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

pub use arcs::MakeArcsConfig;

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
    pub omit_repeated_commands: bool,
    /// Strip leading zeros from decimals in `(-1, 1)` (`0.5` -> `.5`) (`SVGO leadingZero`).
    pub leading_zero: bool,
    /// Convert circular quadratic Bézier runs into SVG `A`/`a` arcs (`SVGO makeArcs`).
    pub make_arcs: Option<MakeArcsConfig>,
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
            omit_repeated_commands: false,
            leading_zero: false,
            make_arcs: None,
        }
    }

    /// Try to produce a compact path.
    ///
    /// Applies the baseline optimizations from [Svgo convertPathData.js](https://github.com/Svg/Svgo/blob/main/plugins/convertPathData.js):
    /// choosing the shorter of absolute vs. relative coordinates, `H`/`V` line shorthands,
    /// `T`/`S` smooth curve shorthands, and removing zero-length no-op commands.
    pub const fn compact(precision: usize) -> Self {
        Self {
            precision,
            utilize_absolute: true,
            line_shorthands: true,
            curve_smooth_shorthands: true,
            remove_useless: true,
            convert_to_z: false,
            collapse_collinear: false,
            omit_repeated_commands: false,
            leading_zero: false,
            make_arcs: None,
        }
    }

    /// Produce a maximally minimized SVG path by enabling all path optimizations, including
    /// redundant closing-line removal, collinear line merging, repeated command omission,
    /// leading-zero stripping, and circular arc fitting.
    pub const fn minimized(precision: usize) -> Self {
        Self {
            precision,
            utilize_absolute: true,
            line_shorthands: true,
            curve_smooth_shorthands: true,
            remove_useless: true,
            convert_to_z: true,
            collapse_collinear: true,
            omit_repeated_commands: true,
            leading_zero: true,
            make_arcs: Some(MakeArcsConfig::lossy()),
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
    pub const fn with_omit_repeated_commands(mut self, enabled: bool) -> Self {
        self.omit_repeated_commands = enabled;
        self
    }

    /// Enable or disable stripping leading zeros from decimals in `(-1, 1)`.
    pub const fn with_leading_zero(mut self, enabled: bool) -> Self {
        self.leading_zero = enabled;
        self
    }

    /// Configure or disable converting circular quadratic Bézier runs into SVG `A`/`a` arcs.
    pub const fn with_make_arcs(mut self, make_arcs: Option<MakeArcsConfig>) -> Self {
        self.make_arcs = make_arcs;
        self
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
        let mut elements = path.elements().to_vec();
        if self.remove_useless {
            elements = remove_useless_segments(elements, *self);
        }
        if self.collapse_collinear {
            elements = collapse_collinear_lines(elements, *self);
        }
        if self.convert_to_z {
            elements = convert_closing_lines_to_z(elements, *self);
        }
        write_formatted_path(&elements, draw_type, *self)
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
        if draw_type == DrawingCommandType::Svg && path_style.leading_zero {
            if let Some(rest) = val.strip_prefix("0.") {
                val = format!(".{rest}");
            } else if let Some(rest) = val.strip_prefix("-0.") {
                val = format!("-.{rest}");
            }
        }
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
                if y_str.starts_with('-') {
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

fn split_subpaths(elements: &[PathEl]) -> Vec<Vec<PathEl>> {
    let mut subpaths = Vec::new();
    let mut current = Vec::new();
    for &el in elements {
        if matches!(el, PathEl::MoveTo(_)) && !current.is_empty() {
            subpaths.push(current);
            current = vec![el];
        } else {
            current.push(el);
        }
    }
    if !current.is_empty() {
        subpaths.push(current);
    }
    subpaths
}

fn has_draw_segment(subpath: &[PathEl]) -> bool {
    subpath.iter().any(|el| {
        matches!(
            el,
            PathEl::LineTo(_) | PathEl::QuadTo(_, _) | PathEl::CurveTo(_, _, _)
        )
    })
}

/// Drops zero-length segments (after rounding to `path_style.precision`).
fn remove_useless_segments(elements: Vec<PathEl>, path_style: SvgPathStyle) -> Vec<PathEl> {
    let mut out = Vec::with_capacity(elements.len());
    let mut sub_start = Point::default();
    let mut curr = Point::default();
    for el in elements {
        match el {
            PathEl::MoveTo(p) => {
                out.push(PathEl::MoveTo(p));
                sub_start = p;
                curr = p;
            }
            PathEl::LineTo(p) => {
                if !path_style.round_eq(curr, p) {
                    out.push(PathEl::LineTo(p));
                    curr = p;
                }
            }
            PathEl::QuadTo(p1, p2) => {
                if !path_style.round_eq(curr, p2) {
                    out.push(PathEl::QuadTo(p1, p2));
                    curr = p2;
                }
            }
            PathEl::CurveTo(p1, p2, p3) => {
                if !path_style.round_eq(curr, p3) {
                    out.push(PathEl::CurveTo(p1, p2, p3));
                    curr = p3;
                }
            }
            PathEl::ClosePath => {
                out.push(PathEl::ClosePath);
                curr = sub_start;
            }
        }
    }
    out
}

/// Merges consecutive collinear [`PathEl::LineTo`] segments pointing in the same direction.
fn collapse_collinear_lines(elements: Vec<PathEl>, path_style: SvgPathStyle) -> Vec<PathEl> {
    let prec = path_style.precision();
    let mut out = Vec::with_capacity(elements.len());
    let mut sub_start = Point::default();
    let mut prev_start = Point::default();
    let mut curr = Point::default();

    for el in elements {
        match el {
            PathEl::MoveTo(p) => {
                let pr = p.round_prec(prec);
                sub_start = pr;
                prev_start = pr;
                curr = pr;
                out.push(PathEl::MoveTo(p));
            }
            PathEl::LineTo(p) => {
                let pr = p.round_prec(prec);
                if pr != curr {
                    if let Some(PathEl::LineTo(prev_end)) = out.last_mut() {
                        let prev_end_r = prev_end.round_prec(prec);
                        let v1 = prev_end_r - prev_start;
                        let v2 = pr - prev_end_r;
                        if v1.cross(v2).abs() < 1e-6 && v1.dot(v2) > 0.0 {
                            *prev_end = p;
                            curr = pr;
                            continue;
                        }
                    }
                }
                prev_start = curr;
                curr = pr;
                out.push(PathEl::LineTo(p));
            }
            PathEl::QuadTo(p1, p2) => {
                prev_start = curr;
                curr = p2.round_prec(prec);
                out.push(PathEl::QuadTo(p1, p2));
            }
            PathEl::CurveTo(p1, p2, p3) => {
                prev_start = curr;
                curr = p3.round_prec(prec);
                out.push(PathEl::CurveTo(p1, p2, p3));
            }
            PathEl::ClosePath => {
                prev_start = sub_start;
                curr = sub_start;
                out.push(PathEl::ClosePath);
            }
        }
    }
    out
}

/// Strips redundant closing [`PathEl::LineTo`] segments immediately before [`PathEl::ClosePath`]
/// when they return to the subpath start.
fn convert_closing_lines_to_z(elements: Vec<PathEl>, path_style: SvgPathStyle) -> Vec<PathEl> {
    let mut out = Vec::with_capacity(elements.len());
    for subpath in split_subpaths(&elements) {
        let Some(&PathEl::MoveTo(sub_start)) = subpath.first() else {
            continue;
        };
        let mut converted = Vec::with_capacity(subpath.len());
        for &el in &subpath {
            if el == PathEl::ClosePath
                && matches!(
                    converted.last(),
                    Some(&PathEl::LineTo(p)) if path_style.round_eq(p, sub_start)
                )
            {
                converted.pop();
            }
            converted.push(el);
        }
        if !path_style.remove_useless || has_draw_segment(&converted) {
            out.extend(converted);
        }
    }
    out
}

fn implied_control(prior_control: Point, prior_end: Point) -> Point {
    // The implied control is the reflection of the prior control over the prior end
    prior_control + 2.0 * (prior_end - prior_control)
}

fn format_svg_cmd(
    last_cmd: Option<&'static str>,
    cmd: &'static str,
    coords: &str,
    omit_repeated_commands: bool,
) -> String {
    let can_omit = omit_repeated_commands
        && cmd != "M"
        && cmd != "m"
        && (last_cmd == Some(cmd)
            || (last_cmd == Some("M") && cmd == "L")
            || (last_cmd == Some("m") && cmd == "l"));
    if can_omit {
        if coords.starts_with('-') {
            coords.to_string()
        } else {
            format!(" {coords}")
        }
    } else {
        format!("{cmd}{coords}")
    }
}

#[derive(Clone)]
struct PathWriter {
    out: String,
    curr: Point,
    subpath_start: Point,
    last_cmd: Option<&'static str>,
    prev_quad: Option<(Point, Point)>,
    prev_cubic: Option<(Point, Point)>,
    draw_type: DrawingCommandType,
    style: SvgPathStyle,
}

impl PathWriter {
    fn new(draw_type: DrawingCommandType, style: SvgPathStyle) -> Self {
        Self {
            out: String::new(),
            curr: Point::default(),
            subpath_start: Point::default(),
            last_cmd: None,
            prev_quad: None,
            prev_cubic: None,
            draw_type,
            style,
        }
    }

    fn emit_formatted(
        &mut self,
        command: DrawingCommand,
        abs_coords: &str,
        rel_coords: Option<&str>,
        is_move: bool,
    ) {
        match self.draw_type {
            DrawingCommandType::Kt => {
                self.out.push_str(self.draw_type.padding());
                if rel_coords.map(|s| s.len()).unwrap_or(usize::MAX) < abs_coords.len() {
                    self.out.push_str(command.rel);
                    self.out.push_str(rel_coords.unwrap());
                } else {
                    self.out.push_str(command.abs);
                    self.out.push_str(abs_coords);
                }
            }
            DrawingCommandType::Svg => {
                let omit = self.style.omit_repeated_commands && !is_move;
                let s_abs = format_svg_cmd(self.last_cmd, command.abs, abs_coords, omit);
                let Some(rel_c) = rel_coords else {
                    self.out.push_str(&s_abs);
                    self.last_cmd = Some(command.abs);
                    return;
                };
                let s_rel = format_svg_cmd(self.last_cmd, command.rel, rel_c, omit);
                let prefer_rel = if omit {
                    s_rel.len() <= s_abs.len()
                } else {
                    s_rel.len() < s_abs.len()
                };
                if prefer_rel {
                    self.out.push_str(&s_rel);
                    self.last_cmd = Some(command.rel);
                } else {
                    self.out.push_str(&s_abs);
                    self.last_cmd = Some(command.abs);
                }
            }
        }
    }

    fn emit_command<T: ToSvgCoord + Copy, const N: usize>(
        &mut self,
        command: DrawingCommand,
        coords: [T; N],
        relative_to: Option<T>,
        is_move: bool,
    ) {
        let abs_coords = self.draw_type.collect_coords(
            coords
                .iter()
                .map(|p| p.write_absolute_coord(self.style, self.draw_type)),
        );
        let rel_coords = relative_to.map(|rel_to| {
            self.draw_type.collect_coords(
                coords
                    .iter()
                    .map(|p| p.write_relative_coord(rel_to, self.style, self.draw_type)),
            )
        });
        self.emit_formatted(command, &abs_coords, rel_coords.as_deref(), is_move);
    }

    fn move_to(&mut self, p: Point) {
        let rel_to = (!self.out.is_empty() && self.style.utilize_absolute).then_some(self.curr);
        self.emit_command(self.draw_type.move_cmd(), [p], rel_to, true);
        self.subpath_start = p;
        self.curr = p;
        self.prev_quad = None;
        self.prev_cubic = None;
    }

    fn line_to(&mut self, p: Point) {
        let rel_to = self.style.utilize_absolute.then_some(self.curr);
        if self.style.line_shorthands && self.style.round_eq(p.x, self.curr.x) {
            self.emit_command(
                self.draw_type.vertical_line_cmd(),
                [p.y],
                rel_to.map(|c| c.y),
                false,
            );
        } else if self.style.line_shorthands && self.style.round_eq(p.y, self.curr.y) {
            self.emit_command(
                self.draw_type.horizontal_line_cmd(),
                [p.x],
                rel_to.map(|c| c.x),
                false,
            );
        } else {
            self.emit_command(self.draw_type.line_cmd(), [p], rel_to, false);
        }
        self.curr = p;
        self.prev_quad = None;
        self.prev_cubic = None;
    }

    fn quad_to(&mut self, p1: Point, p2: Point) {
        let rel_to = self.style.utilize_absolute.then_some(self.curr);
        let is_smooth = self.style.curve_smooth_shorthands
            && matches!(
                self.prev_quad,
                Some((prev_p1, prev_p2))
                    if self.style.round_eq(implied_control(prev_p1, prev_p2), p1)
            );
        if is_smooth {
            self.emit_command(self.draw_type.smooth_quad_cmd(), [p2], rel_to, false);
        } else {
            self.emit_command(self.draw_type.quad_cmd(), [p1, p2], rel_to, false);
        }
        self.curr = p2;
        self.prev_quad = Some((p1, p2));
        self.prev_cubic = None;
    }

    fn curve_to(&mut self, p1: Point, p2: Point, p3: Point) {
        let rel_to = self.style.utilize_absolute.then_some(self.curr);
        let is_smooth = self.style.curve_smooth_shorthands
            && matches!(
                self.prev_cubic,
                Some((prev_p2, prev_p3))
                    if self.style.round_eq(implied_control(prev_p2, prev_p3), p1)
            );
        if is_smooth {
            self.emit_command(self.draw_type.smooth_curve_cmd(), [p2, p3], rel_to, false);
        } else {
            self.emit_command(self.draw_type.curve_cmd(), [p1, p2, p3], rel_to, false);
        }
        self.curr = p3;
        self.prev_quad = None;
        self.prev_cubic = Some((p2, p3));
    }

    fn arc_to(&mut self, arc: CircularArc) {
        let r_str = arc
            .radius
            .write_absolute_coord(self.style, DrawingCommandType::Svg);
        let radii = format!("{r_str},{r_str}");
        let flags = format!("{},{}", u8::from(arc.large_arc), u8::from(arc.sweep));
        let abs_c = DrawingCommandType::Svg.collect_coords(
            [
                radii.clone(),
                "0".into(),
                flags.clone(),
                arc.end
                    .write_absolute_coord(self.style, DrawingCommandType::Svg),
            ]
            .into_iter(),
        );
        let rel_c = self.style.utilize_absolute.then(|| {
            DrawingCommandType::Svg.collect_coords(
                [
                    radii,
                    "0".into(),
                    flags,
                    arc.end
                        .write_relative_coord(self.curr, self.style, DrawingCommandType::Svg),
                ]
                .into_iter(),
            )
        });
        self.emit_formatted(self.draw_type.arc_cmd(), &abs_c, rel_c.as_deref(), false);
        self.curr = arc.end;
        self.prev_quad = None;
        self.prev_cubic = None;
    }

    fn arc_run(&mut self, arcs: &[CircularArc], fallback_quads: &[(Point, Point)]) {
        let mut arc_trial = self.clone();
        for &arc in arcs {
            arc_trial.arc_to(arc);
        }
        let mut quad_trial = self.clone();
        for &(p1, p2) in fallback_quads {
            quad_trial.quad_to(p1, p2);
        }
        *self = if arc_trial.out.len() < quad_trial.out.len() {
            arc_trial
        } else {
            quad_trial
        };
    }

    fn quad_run(&mut self, quads: &[(Point, Point)], arc_opts: MakeArcsConfig) {
        let prec = self.style.precision();
        let mut bez_quads = Vec::with_capacity(quads.len());
        let mut curr = self.curr;
        for &(p1, p2) in quads {
            bez_quads.push(QuadBez::new(curr, p1, p2));
            curr = p2;
        }

        let mut start = 0;
        while start < quads.len() {
            let mut matched_arc = None;
            for end in (start + 2..=quads.len()).rev() {
                if let Some(arcs) = arcs::try_fit_arc_run(&bez_quads[start..end], arc_opts, prec) {
                    matched_arc = Some((end, arcs));
                    break;
                }
            }

            if let Some((end, arcs)) = matched_arc {
                self.arc_run(&arcs, &quads[start..end]);
                start = end;
            } else {
                let (p1, p2) = quads[start];
                self.quad_to(p1, p2);
                start += 1;
            }
        }
    }

    fn close_path(&mut self) {
        // See <https://github.com/harfbuzz/harfbuzz/blob/2da79f70a1d562d883bdde5b74f6603374fb7023/src/hb-draw.hh#L148-L150>
        let needs_closing_line = if self.style.convert_to_z {
            false
        } else if self.style.remove_useless {
            !self.style.round_eq(self.curr, self.subpath_start)
        } else {
            self.curr != self.subpath_start
        };
        if needs_closing_line {
            self.line_to(self.subpath_start);
        }
        let close_cmd = self.draw_type.close_cmd().abs;
        self.out.push_str(close_cmd);
        self.curr = self.subpath_start;
        self.last_cmd = Some(close_cmd);
        self.prev_quad = None;
        self.prev_cubic = None;
    }
}

fn write_formatted_path(
    elements: &[PathEl],
    draw_type: DrawingCommandType,
    path_style: SvgPathStyle,
) -> String {
    let mut writer = PathWriter::new(draw_type, path_style);
    let mut i = 0;
    while i < elements.len() {
        match elements[i] {
            PathEl::MoveTo(p) => {
                writer.move_to(p);
                i += 1;
            }
            PathEl::LineTo(p) => {
                writer.line_to(p);
                i += 1;
            }
            PathEl::QuadTo(p1, p2) => {
                if let (DrawingCommandType::Svg, Some(arc_opts)) = (draw_type, path_style.make_arcs)
                {
                    let mut quads = Vec::new();
                    while let Some(&PathEl::QuadTo(qp1, qp2)) = elements.get(i) {
                        quads.push((qp1, qp2));
                        i += 1;
                    }
                    writer.quad_run(&quads, arc_opts);
                } else {
                    writer.quad_to(p1, p2);
                    i += 1;
                }
            }
            PathEl::CurveTo(p1, p2, p3) => {
                writer.curve_to(p1, p2, p3);
                i += 1;
            }
            PathEl::ClosePath => {
                writer.close_path();
                i += 1;
            }
        }
    }
    writer.out
}

fn round_coord(pt: f64, precision: usize) -> String {
    let rounded = pt.round_prec(precision);
    let mut s = format!("{}", if rounded == 0.0 { 0.0 } else { rounded });

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
