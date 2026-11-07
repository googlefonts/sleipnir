//! Rasterization and image comparison helpers built on tiny-skia.

use anyhow::{anyhow, Context, Result};
use kurbo::{BezPath, PathEl};
use tiny_skia::{FillRule, IntSize, Paint, PathBuilder, Pixmap, Transform};

/// Alpha difference above which a pixel counts as "visibly changed".
pub const VISIBLE_DELTA: u8 = 32;

/// Returns the contents of every `d="..."` attribute in `svg`.
pub fn d_strings(svg: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = svg;
    while let Some(i) = rest.find(" d=\"") {
        let after = &rest[i + 4..];
        let j = after.find('"').unwrap_or(after.len());
        out.push(&after[..j]);
        rest = &after[j..];
    }
    out
}

/// Parses the `viewBox` attribute of `svg` as `[x, y, width, height]`.
pub fn view_box(svg: &str) -> Option<[f32; 4]> {
    let start = svg.find("viewBox=\"")? + "viewBox=\"".len();
    let end = start + svg[start..].find('"')?;
    let vals: Vec<f32> = svg[start..end]
        .split([' ', ','])
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().ok())
        .collect::<Option<_>>()?;
    vals.try_into().ok()
}

/// The filled outline of a single-color SVG icon, ready to rasterize.
pub struct Shape {
    path: Option<tiny_skia::Path>,
    view_box: [f32; 4],
}

impl Shape {
    /// Builds a shape from the `<path>` elements of an SVG produced by `draw_icon`.
    ///
    /// Only plain filled paths are supported; transforms, gradients and strokes are ignored.
    pub fn from_svg(svg: &str) -> Result<Self> {
        let view_box = view_box(svg).context("SVG has no viewBox")?;
        let mut pb = PathBuilder::new();
        for d in d_strings(svg) {
            let path = BezPath::from_svg(d).map_err(|e| anyhow!("invalid path data: {e}"))?;
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
        }
        Ok(Self {
            path: pb.finish(),
            view_box,
        })
    }

    /// Renders the whole icon into a `size`×`size` pixmap, scaled so the viewBox height fits.
    pub fn render(&self, size: u32) -> Pixmap {
        self.render_window(size as f32, 0.0, 0.0, size, size)
    }

    /// Renders the icon scaled to `icon_px` tall, offset by `(-x0, -y0)` pixels, into a
    /// `width`×`height` pixmap. Useful for zoomed-in crops.
    pub fn render_window(&self, icon_px: f32, x0: f32, y0: f32, width: u32, height: u32) -> Pixmap {
        let mut pm = Pixmap::new(width, height).expect("non-zero pixmap size");
        let Some(path) = &self.path else {
            return pm;
        };
        let [vx, vy, _, vh] = self.view_box;
        let s = icon_px / vh;
        let transform = Transform::from_row(s, 0.0, 0.0, s, -vx * s - x0, -vy * s - y0);
        let mut paint = Paint::default();
        paint.set_color_rgba8(0, 0, 0, 255);
        paint.anti_alias = true;
        pm.fill_path(path, &paint, FillRule::Winding, transform, None);
        pm
    }
}

/// Extracts the alpha channel of a pixmap.
pub fn alpha(pm: &Pixmap) -> Vec<u8> {
    pm.pixels().iter().map(|p| p.alpha()).collect()
}

/// Per-image comparison metrics between two alpha masks.
#[derive(Clone, Copy, Default, Debug)]
pub struct Q {
    /// Mean absolute alpha difference per pixel (0–255).
    pub mae: f64,
    /// Largest single-pixel alpha difference.
    pub max: u8,
    /// Fraction of pixels whose alpha changes by more than [`VISIBLE_DELTA`].
    pub changed: f64,
    /// Intersection over union of pixels with alpha > 127.
    pub iou: f64,
}

/// Mean absolute difference between two alpha masks.
pub fn mae(a: &[u8], b: &[u8]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(&x, &y)| x.abs_diff(y) as u64)
        .sum::<u64>() as f64
        / a.len() as f64
}

/// Computes all [`Q`] metrics between two alpha masks of equal size.
pub fn compare(a: &[u8], b: &[u8]) -> Q {
    let mut sum = 0u64;
    let mut max = 0u8;
    let mut changed = 0usize;
    let (mut inter, mut union) = (0usize, 0usize);
    for (&x, &y) in a.iter().zip(b) {
        let d = x.abs_diff(y);
        sum += d as u64;
        max = max.max(d);
        if d > VISIBLE_DELTA {
            changed += 1;
        }
        let (ix, iy) = (x > 127, y > 127);
        inter += (ix && iy) as usize;
        union += (ix || iy) as usize;
    }
    Q {
        mae: sum as f64 / a.len() as f64,
        max,
        changed: changed as f64 / a.len() as f64,
        iou: if union == 0 {
            1.0
        } else {
            inter as f64 / union as f64
        },
    }
}

/// Diff heatmap: light-grey ghost of `reference`, red where coverage was lost, blue where added.
pub fn heatmap(reference: &Pixmap, other: &Pixmap) -> Pixmap {
    let (w, h) = (reference.width(), reference.height());
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for (pr, po) in reference.pixels().iter().zip(other.pixels()) {
        let (a, b) = (pr.alpha() as f32, po.alpha() as f32);
        let ghost = 255.0 - a * 0.18;
        let d = (b - a) / 255.0;
        let t = (d.abs() * 1.6).min(1.0);
        let target = if d < 0.0 {
            [220.0, 20.0, 40.0]
        } else {
            [20.0, 90.0, 230.0]
        };
        let mix = |i: usize| (ghost * (1.0 - t) + target[i] * t) as u8;
        data.extend_from_slice(&[mix(0), mix(1), mix(2), 255]);
    }
    Pixmap::from_vec(data, IntSize::from_wh(w, h).expect("non-zero size")).expect("valid pixmap")
}

/// Encodes a pixmap as a `data:` URI for embedding in HTML.
pub fn png_data_uri(pm: &Pixmap) -> String {
    let png = pm.encode_png().expect("PNG encoding");
    format!("data:image/png;base64,{}", base64(&png))
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn parses_view_box_and_paths() {
        let svg =
            r#"<svg viewBox="0 -960 960 960"><path d="M0,0L10,0L10-10Z"/><path d="M1,1"/></svg>"#;
        assert_eq!(view_box(svg), Some([0.0, -960.0, 960.0, 960.0]));
        assert_eq!(d_strings(svg), vec!["M0,0L10,0L10-10Z", "M1,1"]);
    }

    #[test]
    fn identical_shapes_compare_equal() {
        let svg = r#"<svg viewBox="0 -960 960 960"><path d="M100-100H860V-860H100Z"/></svg>"#;
        let a = alpha(&Shape::from_svg(svg).unwrap().render(24));
        let q = compare(&a, &a);
        assert_eq!((q.mae, q.max, q.changed, q.iou), (0.0, 0, 0.0, 1.0));
        assert!(a.iter().any(|&x| x == 255));
    }
}
