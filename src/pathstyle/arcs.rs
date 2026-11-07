//! Circular arc fitting (`MakeArcsConfig`, `try_fit_arc_run`) and SVG path minimization tests.
//!
//! Detects chains of quadratic Bézier curves (`QuadTo`) that approximate a circular arc or full
//! circle and fits [`CircularArc`] candidates verified against [`kurbo::Arc::from_svg_arc`].

use std::f64::consts::PI;

use crate::pathstyle::Rounding;
use kurbo::{Arc, ParamCurve, Point, QuadBez, SvgArc, Vec2};

/// Configuration for converting circular runs of quadratic Bézier curves into SVG arcs (`A`/`a`).
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct MakeArcsConfig {
    /// Coefficient of the rounding step `10^-precision` (SVGO default: `2.5`; lossy font-unit
    /// default at precision 2: `250.0`, i.e. `2.5` units).
    pub threshold: f64,
    /// Maximum allowed radial deviation as a percentage of the arc radius (SVGO default: `0.5`;
    /// lossy font-unit default: `6.0`).
    pub tolerance: f64,
    /// When `true`, verifies both directions densely (quads -> arc and arc -> quads).
    pub symmetric_check: bool,
}

impl MakeArcsConfig {
    /// Lossy font-coordinate arc fitting (up to `2.5` units at precision 2, capped at `6%` of
    /// the radius for small shapes), with two-way symmetric verification.
    pub const fn lossy() -> Self {
        Self {
            threshold: 250.0,
            tolerance: 6.0,
            symmetric_check: true,
        }
    }

    /// Exact SVGO `convertPathData` `makeArcs` rule (`min(2.5 * 10^-precision, 0.5% * r)`).
    pub const fn svgo() -> Self {
        Self {
            threshold: 2.5,
            tolerance: 0.5,
            symmetric_check: true,
        }
    }

    /// Computes the effective radial tolerance for an arc of radius `r` at `precision`.
    pub fn effective_tolerance(&self, precision: usize, r: f64) -> f64 {
        let error = 10f64.powi(-(precision as i32));
        let base_tol = (self.threshold * error).min(self.tolerance * r / 100.0);
        if r < 25.0 {
            base_tol.min(r * 0.015)
        } else {
            base_tol
        }
    }
}

impl Default for MakeArcsConfig {
    fn default() -> Self {
        Self::lossy()
    }
}

/// A circular SVG `<path>` arc segment (`A r,r 0 large_arc,sweep end.x,end.y`).
#[derive(Clone, Copy, Debug)]
pub(super) struct CircularArc {
    /// Circle radius `r` (`rx == ry == radius`).
    pub radius: f64,
    /// SVG `large-arc-flag` (`true` if the arc spans strictly more than 180°).
    pub large_arc: bool,
    /// SVG `sweep-flag` (`true` for positive/clockwise angle sweep in SVG coordinates).
    pub sweep: bool,
    /// Target endpoint of the arc segment.
    pub end: Point,
}

/// Computes the unique circle `(center, radius)` passing through three non-collinear points
/// `a`, `b`, and `c`. Returns `None` if the three points are collinear (or nearly collinear).
fn circumcircle(a: Point, b: Point, c: Point) -> Option<(Point, f64)> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if d.abs() < 1e-6 {
        return None;
    }
    let a2 = a.x * a.x + a.y * a.y;
    let b2 = b.x * b.x + b.y * b.y;
    let c2 = c.x * c.x + c.y * c.y;
    let cx = (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d;
    let cy = (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d;
    let center = Point::new(cx, cy);
    Some((center, center.distance(a)))
}

/// Returns the signed angular sweep (in radians, wrapped to `[-π, π)`) from `p0` to `p2` around
/// `center`.
fn signed_angle_delta(p0: Point, p2: Point, center: Point) -> f64 {
    let a0 = (p0.y - center.y).atan2(p0.x - center.x);
    let a2 = (p2.y - center.y).atan2(p2.x - center.x);
    (a2 - a0 + PI).rem_euclid(2.0 * PI) - PI
}

/// Verifies that an SVG `<path>` arc command (`A r,r 0 large_arc,sweep end`) reconstructed via
/// [`kurbo::Arc::from_svg_arc`] (which implements W3C SVG 1.1 Appendix F.6.5/F.6.6 endpoint-to-center
/// parameterization) stays within `tol` units of every quadratic Bézier in `quads`.
fn verify_svg_arc(
    quads: &[QuadBez],
    r: f64,
    large_arc: bool,
    sweep: bool,
    tol: f64,
    symmetric: bool,
    precision: usize,
) -> bool {
    let (Some(first), Some(last)) = (quads.first(), quads.last()) else {
        return false;
    };
    let svg_arc = SvgArc {
        from: first.p0.round_prec(precision),
        to: last.p2.round_prec(precision),
        radii: Vec2::new(r, r),
        x_rotation: 0.0,
        large_arc,
        sweep,
    };
    let Some(arc) = Arc::from_svg_arc(&svg_arc) else {
        return false;
    };
    let r_eff = arc.radii.x;
    if (r - r_eff).abs() > tol {
        return false;
    }
    for q in quads {
        if symmetric {
            for i in 1..=16 {
                if (q.eval(i as f64 / 16.0).distance(arc.center) - r_eff).abs() > tol {
                    return false;
                }
            }
        } else {
            for t in [0.2, 0.4, 0.5, 0.6, 0.8, 1.0] {
                if (q.eval(t).distance(arc.center) - r_eff).abs() > tol {
                    return false;
                }
            }
        }
        let da = signed_angle_delta(q.p0, q.p2, arc.center);
        if da.abs() < 0.08 || da.abs() > 1.85 || (da > 0.0) != sweep {
            return false;
        }
    }
    if symmetric {
        let n = 8 * quads.len().max(4);
        for i in 0..=n {
            let a = arc.start_angle + arc.sweep_angle * (i as f64 / n as f64);
            let p = arc.center + Vec2::new(arc.radii.x * a.cos(), arc.radii.y * a.sin());
            let d = quads
                .iter()
                .map(|q| kurbo::ParamCurveNearest::nearest(q, p, 1e-6).distance_sq)
                .fold(f64::INFINITY, f64::min)
                .sqrt();
            if d > tol {
                return false;
            }
        }
    }
    true
}

/// Attempts to fit a single circular SVG arc (`< 360°`) to `quads` within `arc_opts` tolerance.
fn try_fit_partial_arc(
    quads: &[QuadBez],
    arc_opts: MakeArcsConfig,
    precision: usize,
) -> Option<CircularArc> {
    if quads.len() < 2 {
        return None;
    }
    let p_start = quads[0].p0;
    let p_end = quads[quads.len() - 1].p2;
    let chord = p_start
        .round_prec(precision)
        .distance(p_end.round_prec(precision));
    if chord < 1e-4 {
        return None;
    }
    let mid_idx = quads.len() / 2;
    let p_mid = if quads.len() % 2 == 1 {
        quads[mid_idx].eval(0.5)
    } else {
        quads[mid_idx].p0
    };
    let (center, r) = circumcircle(p_start, p_mid, p_end)?;
    let eff_tol = arc_opts.effective_tolerance(precision, r);

    let mut total_angle = 0.0;
    for q in quads {
        let da = signed_angle_delta(q.p0, q.p2, center);
        if da.abs() < 0.08 || da.abs() > 1.85 || (total_angle != 0.0 && da * total_angle <= 0.0) {
            return None;
        }
        total_angle += da;
    }
    if total_angle.abs() > 2.0 * PI - 0.05 {
        return None;
    }

    let sweep = total_angle > 0.0;
    let large_arc = total_angle.abs() > PI + 1e-4;
    let r_chord = (chord / 2.0).floor();
    let r_int = r.round();
    let r_half = (r * 2.0).round() / 2.0;
    let r_prec = r.round_prec(precision);
    let scale = 10f64.powi(precision as i32);
    let r_semi_floor = ((chord / 2.0) * scale).floor() / scale;

    let near_semicircle = (total_angle.abs() - PI).abs() < 0.15;
    let semi_standard = [r_chord, r_int, r_half];
    let semi_small = [r_chord, r_int, r_half, r_semi_floor, r_prec];
    let gen_standard = [r_int, r_half];
    let gen_small = [r_int, r_half, r_prec];
    let candidates: &[f64] = match (near_semicircle, r < 25.0) {
        (true, false) => &semi_standard,
        (true, true) => &semi_small,
        (false, false) => &gen_standard,
        (false, true) => &gen_small,
    };
    for &cand_r in candidates {
        // For near-semicircles, never emit a radius larger than chord / 2, which would draw a
        // shallower minor arc and shift the semicircle center.
        if near_semicircle && cand_r > chord / 2.0 + 1e-9 {
            continue;
        }
        if cand_r > 0.0
            && verify_svg_arc(
                quads,
                cand_r,
                large_arc,
                sweep,
                eff_tol,
                arc_opts.symmetric_check,
                precision,
            )
        {
            return Some(CircularArc {
                radius: cand_r,
                large_arc,
                sweep,
                end: p_end,
            });
        }
    }
    None
}

/// Attempts to fit either a full 360° circle (split into two 180° [`CircularArc`] semicircles,
/// since a single SVG arc with identical start and end points is degenerate) or a single partial
/// circular arc to `quads`.
pub(super) fn try_fit_arc_run(
    quads: &[QuadBez],
    arc_opts: MakeArcsConfig,
    precision: usize,
) -> Option<Vec<CircularArc>> {
    if quads.len() >= 4 && quads[0].p0.distance(quads[quads.len() - 1].p2) < 1e-4 {
        let mid = quads.len() / 2;
        let arc1 = try_fit_partial_arc(&quads[..mid], arc_opts, precision)?;
        let arc2 = try_fit_partial_arc(&quads[mid..], arc_opts, precision)?;
        if (arc1.radius - arc2.radius).abs() < 1e-6 && arc1.sweep == arc2.sweep {
            return Some(vec![arc1, arc2]);
        }
        return None;
    }
    try_fit_partial_arc(quads, arc_opts, precision).map(|arc| vec![arc])
}

#[cfg(test)]
mod tests {
    use crate::iconid::{IconIdentifier, Icons};
    use crate::pathstyle::SvgPathStyle;
    use crate::pens::SvgPathPen;
    use crate::testdata;
    use kurbo::{Affine, BezPath, PathEl};
    use skrifa::instance::Size;
    use skrifa::outline::{pen::PathStyle, DrawSettings};
    use skrifa::{FontRef, MetadataProvider};
    use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Transform};

    /// Rasterizes an SVG `<path d="...">` string (in a `0 -960 960 960` viewBox) onto a `24x24`
    /// anti-aliased `Pixmap`.
    fn rasterize_svg_path(svg_d: &str) -> Pixmap {
        let mut path = BezPath::from_svg(svg_d).expect("valid SVG path");
        // Map font/SVG viewBox (0..960, -960..0) to a 24x24 pixel canvas.
        path.apply_affine(Affine::new([0.025, 0.0, 0.0, 0.025, 0.0, 24.0]));

        let mut pb = PathBuilder::new();
        for el in path.elements() {
            match *el {
                PathEl::MoveTo(p) => pb.move_to(p.x as f32, p.y as f32),
                PathEl::LineTo(p) => pb.line_to(p.x as f32, p.y as f32),
                PathEl::QuadTo(p1, p2) => {
                    pb.quad_to(p1.x as f32, p1.y as f32, p2.x as f32, p2.y as f32)
                }
                PathEl::CurveTo(p1, p2, p3) => pb.cubic_to(
                    p1.x as f32,
                    p1.y as f32,
                    p2.x as f32,
                    p2.y as f32,
                    p3.x as f32,
                    p3.y as f32,
                ),
                PathEl::ClosePath => pb.close(),
            }
        }

        let mut pixmap = Pixmap::new(24, 24).unwrap();
        if let Some(skia_path) = pb.finish() {
            let mut paint = Paint::default();
            paint.set_color(Color::BLACK);
            paint.anti_alias = true;
            pixmap.fill_path(
                &skia_path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
        pixmap
    }

    #[test]
    fn minimized_drops_redundant_close_line_and_degenerate_subpaths() {
        let path = BezPath::from_svg("M160-160L400-160L800-160L800-800L160-800L160-160ZM480-480Z")
            .unwrap();
        assert_eq!(
            SvgPathStyle::minimized(2).write_svg_path(&path),
            "M160-160h640v-640H160Z"
        );
    }

    #[test]
    fn minimized_omits_repeated_commands_and_leading_zeros() {
        let path =
            BezPath::from_svg("M100,100L110,115L95,130Q95.5,130.5 96,131Q96.5,131.5 100,140Z")
                .unwrap();
        assert_eq!(
            SvgPathStyle::minimized(2).write_svg_path(&path),
            "M100,100l10,15-15,15q.5,.5 1,1t4,9Z"
        );
    }

    #[test]
    fn minimized_collapses_quadratic_circles_to_svg_arcs() {
        let more_vert = BezPath::from_svg(
            "M480-160q-33,0-56.5-23.5T400-240t23.5-56.5T480-320t56.5,23.5T560-240t-23.5,56.5T480-160Zm0-240q-33,0-56.5-23.5T400-480t23.5-56.5T480-560t56.5,23.5T560-480t-23.5,56.5T480-400Zm0-240q-33,0-56.5-23.5T400-720t23.5-56.5T480-800t56.5,23.5T560-720t-23.5,56.5T480-640Z",
        )
        .unwrap();
        assert_eq!(
            SvgPathStyle::minimized(2).write_svg_path(&more_vert),
            "M480-160a80,80 0 0,1 0-160 80,80 0 0,1 0,160Zm0-240a80,80 0 0,1 0-160 80,80 0 0,1 0,160Zm0-240a80,80 0 0,1 0-160 80,80 0 0,1 0,160Z"
        );
    }

    #[test]
    fn minimized_preserves_small_synthetic_circles() {
        // 8-quad circle with radius r = 3.5 starting at 0° (previously snapped to r = 4,
        // shifting the semicircle centers by sqrt(4^2 - 3.5^2) = 1.94 units).
        let make_quad_circle = |cx: f64, cy: f64, r: f64, n_quads: usize, start_angle: f64| {
            let mut path = BezPath::new();
            let dtheta = std::f64::consts::TAU / (n_quads as f64);
            let rc = r / (dtheta * 0.5).cos();
            path.move_to((cx + r * start_angle.cos(), cy + r * start_angle.sin()));
            for i in 0..n_quads {
                let a0 = start_angle + (i as f64) * dtheta;
                let a1 = a0 + dtheta;
                let amid = (a0 + a1) * 0.5;
                path.quad_to(
                    (cx + rc * amid.cos(), cy + rc * amid.sin()),
                    (cx + r * a1.cos(), cy + r * a1.sin()),
                );
            }
            path.close_path();
            path
        };

        let circle_3_5 = make_quad_circle(0.0, 0.0, 3.5, 8, 0.0);
        assert_eq!(
            SvgPathStyle::minimized(2).write_svg_path(&circle_3_5),
            "M3.5,0a3.5,3.5 0 0,1-7,0 3.5,3.5 0 0,1 7,0Z"
        );

        let rasterize = |d: &str, min_x: f64, min_y: f64, size: f64| -> Pixmap {
            let mut path = BezPath::from_svg(d).expect("valid SVG path");
            let scale = 64.0 / size;
            path.apply_affine(Affine::new([
                scale,
                0.0,
                0.0,
                scale,
                -min_x * scale,
                -min_y * scale,
            ]));
            let mut pb = PathBuilder::new();
            for el in path.elements() {
                match *el {
                    PathEl::MoveTo(p) => pb.move_to(p.x as f32, p.y as f32),
                    PathEl::LineTo(p) => pb.line_to(p.x as f32, p.y as f32),
                    PathEl::QuadTo(p1, p2) => {
                        pb.quad_to(p1.x as f32, p1.y as f32, p2.x as f32, p2.y as f32)
                    }
                    PathEl::CurveTo(p1, p2, p3) => pb.cubic_to(
                        p1.x as f32,
                        p1.y as f32,
                        p2.x as f32,
                        p2.y as f32,
                        p3.x as f32,
                        p3.y as f32,
                    ),
                    PathEl::ClosePath => pb.close(),
                }
            }
            let mut pixmap = Pixmap::new(64, 64).unwrap();
            if let Some(skia_path) = pb.finish() {
                let mut paint = Paint::default();
                paint.set_color(Color::BLACK);
                paint.anti_alias = true;
                pixmap.fill_path(
                    &skia_path,
                    &paint,
                    FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }
            pixmap
        };

        for r in [3.5_f64, 10.0, 50.0, 100.0, 200.0, 400.0, 480.0] {
            for n_quads in [8usize, 12] {
                for start_deg in [0.0_f64, 30.0, 45.0] {
                    let path = make_quad_circle(480.0, -480.0, r, n_quads, start_deg.to_radians());
                    let d_unchanged = SvgPathStyle::Unchanged(2).write_svg_path(&path);
                    let d_compact = SvgPathStyle::minimized(2).write_svg_path(&path);
                    let pad = r * 1.25;
                    let pix_u = rasterize(&d_unchanged, 480.0 - pad, -480.0 - pad, pad * 2.0);
                    let pix_c = rasterize(&d_compact, 480.0 - pad, -480.0 - pad, pad * 2.0);
                    let max_alpha_diff = pix_u
                        .pixels()
                        .iter()
                        .zip(pix_c.pixels().iter())
                        .map(|(u, c)| u.alpha().abs_diff(c.alpha()))
                        .max()
                        .unwrap_or(0);
                    let mean_alpha_diff: f64 = pix_u
                        .pixels()
                        .iter()
                        .zip(pix_c.pixels().iter())
                        .map(|(u, c)| f64::from(u.alpha().abs_diff(c.alpha())) / 255.0)
                        .sum::<f64>()
                        / (64.0 * 64.0);
                    assert!(
                        max_alpha_diff <= 96 && mean_alpha_diff < 0.01,
                        "circle r={r} ({n_quads} quads, start {start_deg}°) had max_alpha_diff={max_alpha_diff}, mean_alpha_diff={mean_alpha_diff:.4}: {d_compact}"
                    );
                }
            }
        }
    }

    #[test]
    fn compact_matches_unchanged_visually_across_icon_fonts() {
        let fonts = [
            (
                "MATERIAL_SYMBOLS_POPULAR",
                testdata::MATERIAL_SYMBOLS_POPULAR,
            ),
            ("ICON_FONT", testdata::ICON_FONT),
            ("FULL_VF_OLD", testdata::FULL_VF_OLD),
            ("FULL_VF_NEW", testdata::FULL_VF_NEW),
            ("PLAY_ARROW_VF", testdata::PLAY_ARROW_VF),
        ];
        let axis_configs: &[&[(&str, f32)]] = &[
            &[],
            &[("FILL", 1.0)],
            &[("wght", 100.0), ("opsz", 20.0)],
            &[("wght", 700.0), ("opsz", 48.0), ("FILL", 1.0)],
        ];
        let mut total_icons = 0usize;
        let mut total_unchanged_bytes = 0usize;
        let mut total_compact_bytes = 0usize;

        for (font_name, font_bytes) in fonts {
            let font = FontRef::new(font_bytes).unwrap();
            let outlines = font.outline_glyphs();
            for axes in axis_configs {
                let loc = font.axes().location(*axes);
                for icon in font.icons().unwrap() {
                    let name = icon.names.first().cloned().unwrap_or_default();
                    let gid = IconIdentifier::GlyphId(icon.gid)
                        .resolve(&font, &loc)
                        .unwrap();
                    let Some(outline) = outlines.get(gid) else {
                        continue;
                    };
                    let mut pen = SvgPathPen::new_with_transform(Affine::new([
                        1.0, 0.0, 0.0, -1.0, 0.0, 0.0,
                    ]));
                    outline
                        .draw(
                            DrawSettings::unhinted(Size::unscaled(), &loc)
                                .with_path_style(PathStyle::HarfBuzz),
                            &mut pen,
                        )
                        .unwrap();
                    let path = pen.into_inner();
                    if path.elements().is_empty() {
                        continue;
                    }

                    let unchanged_d = SvgPathStyle::Unchanged(2).write_svg_path(&path);
                    let compact_d = SvgPathStyle::minimized(2).write_svg_path(&path);

                    assert!(
                        compact_d.len() <= unchanged_d.len(),
                        "{font_name}:{name}: compact ({}) longer than unchanged ({})",
                        compact_d.len(),
                        unchanged_d.len()
                    );

                    let unchanged_pix = rasterize_svg_path(&unchanged_d);
                    let compact_pix = rasterize_svg_path(&compact_d);
                    let max_alpha_diff = unchanged_pix
                        .pixels()
                        .iter()
                        .zip(compact_pix.pixels().iter())
                        .map(|(a, b)| a.alpha().abs_diff(b.alpha()))
                        .max()
                        .unwrap_or(0);
                    let mean_alpha_diff: f64 = unchanged_pix
                        .pixels()
                        .iter()
                        .zip(compact_pix.pixels().iter())
                        .map(|(a, b)| f64::from(a.alpha().abs_diff(b.alpha())) / 255.0)
                        .sum::<f64>()
                        / (24.0 * 24.0);

                    // Anti-aliased subpixel coverage difference from circular arc fitting
                    // (<= 0.0625px at 24x24) must stay tiny across every rendered icon.
                    assert!(
                        max_alpha_diff <= 48 && mean_alpha_diff < 0.015,
                        "{font_name}:{name}@{axes:?}: visual mismatch (max_alpha_diff={max_alpha_diff}/255, mean_alpha_diff={mean_alpha_diff:.4})\nunchanged: {unchanged_d}\ncompact:   {compact_d}"
                    );

                    total_icons += 1;
                    total_unchanged_bytes += unchanged_d.len();
                    total_compact_bytes += compact_d.len();
                }
            }
        }

        assert_eq!(total_icons, 140);
        assert!(
            total_compact_bytes < total_unchanged_bytes,
            "expected compact ({total_compact_bytes}) < unchanged ({total_unchanged_bytes})"
        );
    }

    #[test]
    fn style_toggles_control_individual_optimizations() {
        let mut path = BezPath::new();
        path.move_to((0.5, 0.0));
        path.line_to((0.5, 0.0)); // zero-length segment
        path.line_to((10.5, 0.0)); // horizontal line
        path.line_to((20.5, 0.0)); // collinear horizontal line
        path.line_to((20.5, 10.0)); // vertical line
        path.line_to((0.5, 0.0)); // closing line back to start
        path.close_path();

        assert_eq!(
            SvgPathStyle::minimized(2).write_svg_path(&path),
            "M.5,0h20v10Z"
        );
        assert_eq!(
            SvgPathStyle::minimized(2)
                .with_leading_zero(false)
                .write_svg_path(&path),
            "M0.5,0h20v10Z"
        );
        assert_eq!(
            SvgPathStyle::minimized(2)
                .with_line_shorthands(false)
                .write_svg_path(&path),
            "M.5,0l20,0 0,10Z"
        );
        assert_eq!(
            SvgPathStyle::minimized(2)
                .with_collapse_collinear(false)
                .write_svg_path(&path),
            "M.5,0h10 10v10Z"
        );
        assert_eq!(
            SvgPathStyle::minimized(2)
                .with_collapse_collinear(false)
                .with_omit_repeated_commands(false)
                .write_svg_path(&path),
            "M.5,0h10h10V10Z"
        );
        assert_eq!(
            SvgPathStyle::minimized(2)
                .with_convert_to_z(false)
                .write_svg_path(&path),
            "M.5,0h20v10L.5,0Z"
        );
    }

    #[test]
    fn smooth_shorthands_handle_quantized_reflected_control_points() {
        // After rounding to precision 2, prev p1 = (1.004, 0) -> 1.00, prev p2 = (2.004, 0) -> 2.00,
        // reflected = 3.008 -> 3.01, which differs by 1 ULP (0.01) from 2 * 2.00 - 1.00 = 3.00.
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.quad_to((1.004, 1.0), (2.004, 0.0));
        path.quad_to((3.004, -1.0), (4.004, 0.0));
        path.quad_to((5.004, 1.0), (6.004, 0.0));

        assert_eq!(
            SvgPathStyle::minimized(2)
                .with_make_arcs(None)
                .write_svg_path(&path),
            "M0,0q1,1 2,0t2,0 2,0"
        );
        assert_eq!(
            SvgPathStyle::minimized(2)
                .with_make_arcs(None)
                .with_curve_smooth_shorthands(false)
                .write_svg_path(&path),
            "M0,0q1,1 2,0 1-1 2,0 1,1 2,0"
        );
    }
}
