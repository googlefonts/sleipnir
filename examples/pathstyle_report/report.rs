//! HTML and JSON rendering of the collected measurements.

use crate::raster::{compare, d_strings, heatmap, png_data_uri, Shape};
use crate::{draw, Ctx, FontData, QualRec, Rec};
use anyhow::Result;
use skrifa::FontRef;
use std::collections::HashSet;
use std::fmt::Write as _;

const CSS: &str = include_str!("report.css");

fn pct(a: f64, b: f64) -> f64 {
    if a == 0.0 {
        0.0
    } else {
        (a - b) / a * 100.0
    }
}

fn quantile(v: &mut [f64], q: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * q).round() as usize]
}

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        f64::NAN
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn fmt_bytes(b: usize) -> String {
    let f = b as f64;
    if f >= 1e6 {
        format!("{:.2} MB", f / 1e6)
    } else if f >= 1e3 {
        format!("{:.1} kB", f / 1e3)
    } else {
        format!("{b} B")
    }
}

/// Formats a signed percentage change, coloring savings green and growth red.
fn delta(p: f64) -> String {
    if p >= 0.0 {
        format!(r#"<span class="good">−{p:.1}%</span>"#)
    } else {
        format!(r#"<span class="bad">+{:.1}%</span>"#, -p)
    }
}

fn histogram_svg(values: &[f64], lo: f64, hi: f64, step: f64, xlabel: &str) -> String {
    let nb = ((hi - lo) / step).round() as usize;
    let mut bins = vec![0usize; nb + 2];
    for &v in values {
        let i = if v < lo {
            0
        } else if v >= hi {
            nb + 1
        } else {
            1 + ((v - lo) / step) as usize
        };
        bins[i.min(nb + 1)] += 1;
    }
    let max = bins.iter().copied().max().unwrap_or(1).max(1) as f64;
    let (w, h, pl, pb) = (900.0, 220.0, 50.0, 40.0);
    let bw = (w - pl - 10.0) / (nb + 2) as f64;
    let mut s = format!(r#"<svg viewBox="0 0 {w} {h}" width="100%" style="max-width:{w}px">"#);
    for (i, &c) in bins.iter().enumerate() {
        let bh = (c as f64 / max) * (h - pb - 10.0);
        let x = pl + i as f64 * bw;
        let edge = i == 0 || i == nb + 1;
        let label = if i == 0 {
            format!("< {lo}")
        } else if i == nb + 1 {
            format!("≥ {hi}")
        } else {
            let a = lo + (i - 1) as f64 * step;
            format!("{a}–{}", a + step)
        };
        let _ = write!(
            s,
            r#"<rect x="{x:.1}" y="{:.1}" width="{:.1}" height="{bh:.1}" fill="{}"><title>{label}: {c}</title></rect>"#,
            h - pb - bh,
            bw - 1.0,
            if edge { "#8c959f" } else { "#0969da" }
        );
        if edge || i % 2 == 1 {
            let tick = if edge {
                label.clone()
            } else {
                format!("{}", lo + (i - 1) as f64 * step)
            };
            let _ = write!(
                s,
                r##"<text x="{:.1}" y="{:.1}" font-size="10" text-anchor="middle" fill="#59636e">{}</text>"##,
                x + bw / 2.0,
                h - pb + 14.0,
                esc(&tick)
            );
        }
    }
    let _ = write!(
        s,
        r##"<line x1="{pl}" y1="{y}" x2="{x2}" y2="{y}" stroke="#d1d9e0"/><text x="{cx}" y="{ly}" font-size="12" text-anchor="middle">{xlabel}</text><text x="{pl}" y="16" font-size="11" fill="#59636e">max bin {mx}</text></svg>"##,
        y = h - pb,
        x2 = w - 10.0,
        cx = (w + pl) / 2.0,
        ly = h - 6.0,
        xlabel = esc(xlabel),
        mx = max as usize
    );
    s
}

fn scatter_svg(points: &[(f64, f64, usize)], font_labels: &[&str], ylabel: &str) -> String {
    const COLORS: [&str; 6] = [
        "#0969da", "#bf3989", "#1a7f37", "#9a6700", "#8250df", "#cf222e",
    ];
    let (w, h, pl, pb) = (900.0, 340.0, 60.0, 44.0);
    let xmax = points.iter().map(|p| p.0).fold(1.0, f64::max);
    let xmin = points.iter().map(|p| p.0).fold(0.0, f64::min);
    let ymax = points.iter().map(|p| p.1).fold(0.01, f64::max);
    let sx = |x: f64| pl + (x - xmin) / (xmax - xmin) * (w - pl - 20.0);
    let sy = |y: f64| h - pb - y / ymax * (h - pb - 30.0);
    let mut s = format!(r#"<svg viewBox="0 0 {w} {h}" width="100%" style="max-width:{w}px">"#);
    for i in 0..=5 {
        let yv = ymax * i as f64 / 5.0;
        let xv = xmin + (xmax - xmin) * i as f64 / 5.0;
        let _ = write!(
            s,
            r##"<line x1="{pl}" x2="{:.1}" y1="{y:.1}" y2="{y:.1}" stroke="#eaeef2"/><text x="{:.1}" y="{:.1}" font-size="10" text-anchor="end" fill="#59636e">{yv:.2}</text><text x="{:.1}" y="{:.1}" font-size="10" text-anchor="middle" fill="#59636e">{xv:.0}</text>"##,
            w - 20.0,
            pl - 6.0,
            sy(yv) + 3.0,
            sx(xv),
            h - pb + 14.0,
            y = sy(yv)
        );
    }
    for &(x, y, f) in points {
        let _ = write!(
            s,
            r#"<circle cx="{:.1}" cy="{:.1}" r="2" fill="{}" fill-opacity="0.45"/>"#,
            sx(x),
            sy(y),
            COLORS[f % COLORS.len()]
        );
    }
    let _ = write!(
        s,
        r##"<text x="{:.1}" y="{:.1}" font-size="12" text-anchor="middle">SVG bytes saved per icon</text><text transform="translate(14 {:.1}) rotate(-90)" font-size="12" text-anchor="middle">{}</text>"##,
        (w + pl) / 2.0,
        h - 8.0,
        (h - pb) / 2.0,
        esc(ylabel)
    );
    for (i, l) in font_labels.iter().enumerate() {
        let x = pl + 10.0 + i as f64 * 260.0;
        let _ = write!(
            s,
            r#"<circle cx="{x:.1}" cy="12" r="5" fill="{}"/><text x="{:.1}" y="16" font-size="12">{}</text>"#,
            COLORS[i % COLORS.len()],
            x + 10.0,
            esc(l)
        );
    }
    s.push_str("</svg>");
    s
}

/// Builds the full HTML report.
pub fn build(ctx: &Ctx, fonts: &[FontData], recs: &[Rec], elapsed: f64) -> Result<String> {
    let [lr, lb, lc] = ctx.labels.each_ref().map(|s| esc(s));
    let sizes = &ctx.sizes;
    let mut h = String::new();
    let _ = write!(
        h,
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Path style report: {lb} → {lc}</title><style>{CSS}</style></head><body><main>"#
    );
    let sampled = recs.iter().filter(|r| r.q.is_some()).count();
    let _ = write!(
        h,
        r#"<h1>Path style impact: <code>{lb}</code> → <code>{lc}</code></h1>
<p class="muted">Baseline <code>{lb}</code> vs candidate <code>{lc}</code>. Sizes are full SVG bytes from <code>draw_icon</code>.
Quality reference is <code>{lr}</code>, rasterized with tiny-skia at {sz} px.
{n} icon × location records; {sampled} rasterized ({sampling}). Generated in {elapsed:.0}s.</p>"#,
        sz = sizes
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(", "),
        n = recs.len(),
        sampling = esc(&ctx.sampling),
    );

    summary(&mut h, ctx, fonts, recs);
    size_section(&mut h, ctx, fonts, recs);
    ablation_section(&mut h, ctx, fonts, recs);
    quality_section(&mut h, ctx, fonts, recs);

    let mut pts: Vec<(f64, f64, usize)> = recs
        .iter()
        .filter_map(|r| {
            let q = r.q.as_ref()?;
            Some((
                r.bytes[1] as f64 - r.bytes[2] as f64,
                q.by_size[0][1].mae,
                r.font,
            ))
        })
        .collect();
    if pts.len() > 5000 {
        let step = pts.len() / 5000 + 1;
        pts = pts.into_iter().step_by(step).collect();
    }
    let _ = write!(h, "<h2>Bytes saved vs quality loss ({} px)</h2>", sizes[0]);
    h.push_str(&scatter_svg(
        &pts,
        &fonts.iter().map(|f| f.label.as_str()).collect::<Vec<_>>(),
        &format!("MAE @{}px vs {}", sizes[0], ctx.labels[0]),
    ));

    examples_section(&mut h, ctx, fonts, recs)?;

    h.push_str("<h2>Notes</h2><ul>");
    for f in fonts {
        let _ = write!(
            h,
            "<li>{}: {} glyphs (excluding .notdef), {} empty outlines skipped, {} draw errors at the default location. Locations: {}.</li>",
            esc(&f.label),
            f.glyph_count,
            f.empty,
            f.errors,
            f.locs.iter().map(|l| esc(&l.label)).collect::<Vec<_>>().join(" · ")
        );
    }
    let _ = write!(
        h,
        r#"<li>MAE = mean absolute alpha difference per pixel (0–255). A pixel is "visibly changed" when its alpha moves by more than {vd}/255. IoU = intersection over union of pixels with alpha &gt; 127.</li>
<li>Axes a font lacks are dropped from a location; locations that become empty or duplicate another are skipped.</li>
<li>Generated by <code>cargo run --release --example pathstyle_report</code>. Aggregates are written next to this file as JSON.</li></ul>"#,
        vd = crate::raster::VISIBLE_DELTA
    );
    h.push_str("<footer>sleipnir examples/pathstyle_report</footer></main></body></html>");
    Ok(h)
}

fn summary(h: &mut String, ctx: &Ctx, fonts: &[FontData], recs: &[Rec]) {
    h.push_str("<h2>Summary</h2><div class=\"cards\">");
    let tb: usize = recs.iter().map(|r| r.bytes[1]).sum();
    let tc: usize = recs.iter().map(|r| r.bytes[2]).sum();
    let _ = write!(
        h,
        r#"<div class="card"><div class="k">All fonts · SVG size</div><div class="v">{}</div><div class="s">{} → {}</div></div>"#,
        delta(pct(tb as f64, tc as f64)),
        fmt_bytes(tb),
        fmt_bytes(tc)
    );
    for (fi, f) in fonts.iter().enumerate() {
        let rs: Vec<&Rec> = recs.iter().filter(|r| r.font == fi).collect();
        let b: usize = rs.iter().map(|r| r.bytes[1]).sum();
        let c: usize = rs.iter().map(|r| r.bytes[2]).sum();
        let arcs = rs.iter().filter(|r| r.arcs > 0).count();
        let _ = write!(
            h,
            r#"<div class="card"><div class="k">{}</div><div class="v">{}</div><div class="s">{} → {}<br>{:.1}% of icons use ≥1 arc</div></div>"#,
            esc(&f.label),
            delta(pct(b as f64, c as f64)),
            fmt_bytes(b),
            fmt_bytes(c),
            arcs as f64 / rs.len().max(1) as f64 * 100.0
        );
    }
    for (si, &size) in ctx.sizes.iter().enumerate() {
        let mut mb: Vec<f64> = recs
            .iter()
            .filter_map(|r| r.q.as_ref())
            .map(|q| q.by_size[si][0].mae)
            .collect();
        let mut mc: Vec<f64> = recs
            .iter()
            .filter_map(|r| r.q.as_ref())
            .map(|q| q.by_size[si][1].mae)
            .collect();
        let _ = write!(
            h,
            r#"<div class="card"><div class="k">Pixel MAE @ {size}px (0–255)</div><div class="v">{:.3}</div><div class="s">{} median · p95 {:.3} · p99.9 {:.3}<br>{} median {:.3} · p95 {:.3}</div></div>"#,
            quantile(&mut mc, 0.5),
            esc(&ctx.labels[2]),
            quantile(&mut mc, 0.95),
            quantile(&mut mc, 0.999),
            esc(&ctx.labels[1]),
            quantile(&mut mb, 0.5),
            quantile(&mut mb, 0.95),
        );
    }
    let _ = write!(
        h,
        r#"</div><p class="muted">MAE is measured against <code>{}</code>.</p>"#,
        esc(&ctx.labels[0])
    );
}

fn size_section(h: &mut String, ctx: &Ctx, fonts: &[FontData], recs: &[Rec]) {
    let [lr, lb, lc] = ctx.labels.each_ref().map(|s| esc(s));
    let _ = write!(
        h,
        r#"<h2>Size</h2><p class="muted">Full SVG bytes as emitted by <code>draw_icon</code>.</p><table><tr><th class="l">Font</th><th class="l">Location</th><th>Icons</th><th>{lr}</th><th>{lb}</th><th>{lc}</th><th>Δ vs baseline</th><th>Mean B/icon</th><th>Icons w/ arcs</th><th>Icons larger</th></tr>"#
    );
    for (fi, f) in fonts.iter().enumerate() {
        for (li, loc) in f.locs.iter().enumerate() {
            let rs: Vec<&Rec> = recs
                .iter()
                .filter(|r| r.font == fi && r.loc == li)
                .collect();
            let n = rs.len().max(1);
            let sum = |i: usize| rs.iter().map(|r| r.bytes[i]).sum::<usize>();
            let (r, b, c) = (sum(0), sum(1), sum(2));
            let p = pct(b as f64, c as f64);
            let larger = rs.iter().filter(|r| r.bytes[2] > r.bytes[1]).count();
            let _ = write!(
                h,
                r#"<tr{}><td>{}</td><td class="l">{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{} <span class="bar" style="width:{:.0}px"></span></td><td>{:.0} → {:.0}</td><td>{:.1}%</td><td class="{}">{larger}</td></tr>"#,
                if li == 0 && fi > 0 {
                    r#" class="sep""#
                } else {
                    ""
                },
                if li == 0 {
                    esc(&f.label)
                } else {
                    String::new()
                },
                esc(&loc.label),
                rs.len(),
                fmt_bytes(r),
                fmt_bytes(b),
                fmt_bytes(c),
                delta(p),
                p.max(0.0) * 2.0,
                b as f64 / n as f64,
                c as f64 / n as f64,
                rs.iter().filter(|r| r.arcs > 0).count() as f64 / n as f64 * 100.0,
                if larger > 0 { "bad" } else { "" },
            );
        }
    }
    h.push_str("</table>");
    let savings: Vec<f64> = recs
        .iter()
        .map(|r| pct(r.bytes[1] as f64, r.bytes[2] as f64))
        .collect();
    h.push_str("<h3>Per-icon SVG size savings</h3>");
    h.push_str(&histogram_svg(
        &savings,
        0.0,
        50.0,
        2.5,
        &format!("% SVG bytes saved vs {}", ctx.labels[1]),
    ));
}

fn ablation_section(h: &mut String, ctx: &Ctx, fonts: &[FontData], recs: &[Rec]) {
    let Some(first) = fonts.first() else {
        return;
    };
    if first.ablation.is_empty() {
        return;
    }
    let _ = write!(
        h,
        r#"<h2>Savings by option</h2><p class="muted">Each option that differs between the styles, switched individually on top of <code>{}</code> at the default location (all glyphs). Contributions are not strictly additive.</p><table><tr><th class="l">Option</th>"#,
        esc(&ctx.labels[1])
    );
    for f in fonts {
        let _ = write!(h, "<th>{}</th>", esc(&f.label));
    }
    let _ = write!(h, "</tr><tr><td>{} baseline</td>", esc(&ctx.labels[1]));
    for f in fonts {
        let _ = write!(h, "<td>{}</td>", fmt_bytes(f.ablation_base));
    }
    h.push_str("</tr>");
    for (i, (name, _)) in first.ablation.iter().enumerate() {
        let _ = write!(h, "<tr><td><code>{name}</code></td>");
        for f in fonts {
            let v = f.ablation[i].1;
            let p = pct(f.ablation_base as f64, v as f64);
            let _ = write!(
                h,
                r#"<td>{} <span class="bar" style="width:{:.0}px"></span> <span class="muted">({})</span></td>"#,
                delta(p),
                p.max(0.0) * 3.0,
                fmt_bytes(f.ablation_base.abs_diff(v))
            );
        }
        h.push_str("</tr>");
    }
    let _ = write!(h, "<tr><td><b>all ({})</b></td>", esc(&ctx.labels[2]));
    for (fi, f) in fonts.iter().enumerate() {
        let c: usize = recs
            .iter()
            .filter(|r| r.font == fi && r.loc == 0)
            .map(|r| r.bytes[2])
            .sum();
        let _ = write!(
            h,
            "<td><b>{}</b></td>",
            delta(pct(f.ablation_base as f64, c as f64))
        );
    }
    h.push_str("</tr></table>");
}

fn quality_section(h: &mut String, ctx: &Ctx, fonts: &[FontData], recs: &[Rec]) {
    let [lr, lb, lc] = ctx.labels.each_ref().map(|s| esc(s));
    let _ = write!(
        h,
        r#"<h2>Quality</h2><p class="muted">Rasterized glyphs compared to <code>{lr}</code>. Each pair is <b>{lb}</b> / <b>{lc}</b>. “Visible” = % of icons with at least one pixel whose alpha changes by more than {vd}/255.</p>"#,
        vd = crate::raster::VISIBLE_DELTA
    );
    for (si, &size) in ctx.sizes.iter().enumerate() {
        let hi_col = si == 0;
        let _ = write!(
            h,
            r#"<h3>{size} px</h3><table><tr><th class="l">Font</th><th class="l">Location</th><th>N</th><th>MAE mean</th><th>MAE p99</th><th>max MAE (cand.)</th><th>worst Δpx (cand.)</th><th>min IoU</th><th>Visible</th>{}</tr>"#,
            if hi_col {
                format!(
                    "<th>MAE vs {} (ref / base / cand)</th>",
                    esc(&ctx.hi_ref_label)
                )
            } else {
                String::new()
            }
        );
        for (fi, f) in fonts.iter().enumerate() {
            for (li, loc) in f.locs.iter().enumerate() {
                let qs: Vec<&QualRec> = recs
                    .iter()
                    .filter(|r| r.font == fi && r.loc == li)
                    .filter_map(|r| r.q.as_deref())
                    .collect();
                let n = qs.len().max(1) as f64;
                let col =
                    |k: usize| -> Vec<f64> { qs.iter().map(|q| q.by_size[si][k].mae).collect() };
                let (mut mb, mut mc) = (col(0), col(1));
                let iou = |k: usize| qs.iter().map(|q| q.by_size[si][k].iou).fold(1.0, f64::min);
                let vis = |k: usize| {
                    qs.iter().filter(|q| q.by_size[si][k].changed > 0.0).count() as f64 / n * 100.0
                };
                let extra = if hi_col {
                    let m = |k: usize| mean(&qs.iter().map(|q| q.mae_hi[k]).collect::<Vec<_>>());
                    format!("<td>{:.3} / {:.3} / {:.3}</td>", m(0), m(1), m(2))
                } else {
                    String::new()
                };
                let _ = write!(
                    h,
                    r#"<tr{}><td>{}</td><td class="l">{}</td><td>{}</td><td>{:.3} / {:.3}</td><td>{:.3} / {:.3}</td><td>{:.2}</td><td>{}</td><td>{:.3} / {:.3}</td><td>{:.1}% / {:.1}%</td>{extra}</tr>"#,
                    if li == 0 && fi > 0 {
                        r#" class="sep""#
                    } else {
                        ""
                    },
                    if li == 0 {
                        esc(&f.label)
                    } else {
                        String::new()
                    },
                    esc(&loc.label),
                    qs.len(),
                    mean(&mb),
                    mean(&mc),
                    quantile(&mut mb, 0.99),
                    quantile(&mut mc, 0.99),
                    mc.iter().copied().fold(0.0, f64::max),
                    qs.iter().map(|q| q.by_size[si][1].max).max().unwrap_or(0),
                    iou(0),
                    iou(1),
                    vis(0),
                    vis(1),
                );
            }
        }
        h.push_str("</table>");
    }
}

fn pick<'a>(used: &mut HashSet<(usize, u32)>, v: Vec<&'a Rec>, n: usize) -> Vec<&'a Rec> {
    let mut out = Vec::new();
    for r in v {
        if out.len() >= n {
            break;
        }
        if used.insert((r.font, r.gid)) {
            out.push(r);
        }
    }
    out
}

fn examples_section(h: &mut String, ctx: &Ctx, fonts: &[FontData], recs: &[Rec]) -> Result<()> {
    let last = ctx.sizes.len() - 1;
    let sampled: Vec<&Rec> = recs.iter().filter(|r| r.q.is_some()).collect();
    let q = |r: &Rec, si: usize| r.q.as_ref().map(|q| q.by_size[si][1]).unwrap_or_default();
    let mut used = HashSet::new();

    let mut by_mae = sampled.clone();
    by_mae.sort_by(|a, b| q(b, 0).mae.total_cmp(&q(a, 0).mae));
    let worst_mae = pick(&mut used, by_mae, 12);
    let mut by_iou = sampled.clone();
    by_iou.sort_by(|a, b| q(a, last).iou.total_cmp(&q(b, last).iou));
    let worst_iou = pick(&mut used, by_iou, 6);
    let mut by_save = sampled.clone();
    by_save.sort_by_key(|r| std::cmp::Reverse(r.bytes[1] as i64 - r.bytes[2] as i64));
    let best_save = pick(&mut used, by_save.clone(), 12);
    let mut typical: Vec<&Rec> = sampled.iter().copied().filter(|r| r.arcs > 0).collect();
    // Deterministic pseudo-random order.
    typical.sort_by_key(|r| {
        (r.gid as u64 * 2_654_435_761 + r.loc as u64 * 40_503 + r.font as u64 * 97) % 1_000_003
    });
    let typical = pick(&mut used, typical, 6);

    let show = [ctx.sizes[ctx.sizes.len() / 2], ctx.sizes[last]];
    let _ = write!(
        h,
        r#"<h2>Examples</h2><p class="legend muted">Each card: reference, baseline and candidate renders plus a diff heatmap (candidate vs reference) at {} px and {} px.<span style="background:#dc1428"></span>coverage lost<span style="background:#145ae6"></span>coverage added</p>"#,
        show[0], show[1]
    );
    let sections = [
        (
            format!("Largest quality loss (MAE @ {} px)", ctx.sizes[0]),
            worst_mae,
        ),
        (format!("Lowest IoU @ {} px", ctx.sizes[last]), worst_iou),
        ("Largest byte savings".to_string(), best_save),
        ("Typical icons using arcs (random)".to_string(), typical),
    ];
    for (title, list) in &sections {
        if list.is_empty() {
            continue;
        }
        let _ = write!(h, "<h3>{}</h3><div class=\"ex\">", esc(title));
        for r in list {
            h.push_str(&example_card(ctx, fonts, r, show)?);
        }
        h.push_str("</div>");
    }
    if let Some(r) = by_save.first() {
        h.push_str(&biggest_saver(ctx, fonts, r)?);
    }
    Ok(())
}

fn draw_all(ctx: &Ctx, fonts: &[FontData], r: &Rec) -> Result<[String; 3]> {
    let f = &fonts[r.font];
    let font = FontRef::new(&f.data)?;
    let coords = &f.locs[r.loc].coords;
    let [a, b, c] = ctx.styles;
    Ok([
        draw(&font, r.gid, coords, a)?,
        draw(&font, r.gid, coords, b)?,
        draw(&font, r.gid, coords, c)?,
    ])
}

fn example_card(ctx: &Ctx, fonts: &[FontData], r: &Rec, show: [u32; 2]) -> Result<String> {
    let f = &fonts[r.font];
    let svgs = draw_all(ctx, fonts, r)?;
    let shapes = svgs
        .iter()
        .map(|s| Shape::from_svg(s))
        .collect::<Result<Vec<_>>>()?;
    let q = r.q.as_ref().expect("examples are sampled");
    let mut s = String::new();
    let _ = write!(
        s,
        r#"<div class="excard"><h4>{} <span class="muted">· {} · {} · gid {}</span></h4><div class="meta">SVG bytes ref/base/cand: {} / {} / <b>{}</b> ({} vs base) · arcs: {} · cand. MAE @{}: {} · IoU@{} {:.4}</div>"#,
        esc(&r.name),
        esc(&f.label),
        esc(&f.locs[r.loc].label),
        r.gid,
        r.bytes[0],
        r.bytes[1],
        r.bytes[2],
        delta(pct(r.bytes[1] as f64, r.bytes[2] as f64)),
        r.arcs,
        ctx.sizes
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join("/"),
        q.by_size
            .iter()
            .map(|p| format!("{:.2}", p[1].mae))
            .collect::<Vec<_>>()
            .join(" / "),
        ctx.sizes[ctx.sizes.len() - 1],
        q.by_size[q.by_size.len() - 1][1].iou,
    );
    for size in show {
        let [pr, pb, pc] = [0, 1, 2].map(|i| shapes[i].render(size));
        let _ = write!(
            s,
            r#"<div class="imgs" style="margin-bottom:6px"><div><img src="{}"><br>reference {size}px</div><div><img src="{}"><br>baseline</div><div><img src="{}"><br>candidate</div><div><img src="{}"><br>diff</div></div>"#,
            png_data_uri(&pr),
            png_data_uri(&pb),
            png_data_uri(&pc),
            png_data_uri(&heatmap(&pr, &pc))
        );
    }
    let _ = write!(
        s,
        "<details><summary>path data</summary><b>{}</b><pre>{}</pre><b>{}</b><pre>{}</pre></details></div>",
        esc(&ctx.labels[1]),
        esc(&d_strings(&svgs[1]).join("\n")),
        esc(&ctx.labels[2]),
        esc(&d_strings(&svgs[2]).join("\n"))
    );
    Ok(s)
}

/// Spotlights the icon with the largest absolute byte savings, zoomed into the region where the
/// candidate deviates most from the reference.
fn biggest_saver(ctx: &Ctx, fonts: &[FontData], r: &Rec) -> Result<String> {
    const BASE: u32 = 1024;
    const WIN: u32 = 64; // 1/16th of the icon.
    const ZOOM: f32 = 8.0; // 64px window -> 512px output.
    let f = &fonts[r.font];
    let svgs = draw_all(ctx, fonts, r)?;
    let shapes = svgs
        .iter()
        .map(|s| Shape::from_svg(s))
        .collect::<Result<Vec<_>>>()?;
    let Some([vbx, vby, _, vbh]) = crate::raster::view_box(&svgs[0]) else {
        return Ok(String::new());
    };

    // Find the window with the most coverage change.
    let ar = crate::raster::alpha(&shapes[0].render(BASE));
    let ac = crate::raster::alpha(&shapes[2].render(BASE));
    let diff: Vec<u64> = ar
        .iter()
        .zip(&ac)
        .map(|(a, b)| a.abs_diff(*b) as u64)
        .collect();
    let (mut best, mut bx, mut by) = (0u64, 0u32, 0u32);
    for y in (0..=BASE - WIN).step_by(8) {
        for x in (0..=BASE - WIN).step_by(8) {
            let s: u64 = (y..y + WIN)
                .map(|yy| {
                    let row = (yy * BASE) as usize;
                    diff[row + x as usize..row + (x + WIN) as usize]
                        .iter()
                        .sum::<u64>()
                })
                .sum();
            if s > best {
                (best, bx, by) = (s, x, y);
            }
        }
    }
    let out = (WIN as f32 * ZOOM) as u32;
    let icon_px = BASE as f32 * ZOOM;
    let (x0, y0) = (bx as f32 * ZOOM, by as f32 * ZOOM);
    let [zr, zb, zc] = [0, 1, 2].map(|i| shapes[i].render_window(icon_px, x0, y0, out, out));
    let zq = compare(&crate::raster::alpha(&zr), &crate::raster::alpha(&zc));

    // Same window in SVG user units for a crisp vector overlay.
    let upp = vbh as f64 / BASE as f64;
    let (vx, vy, vw) = (
        vbx as f64 + bx as f64 * upp,
        vby as f64 + by as f64 * upp,
        WIN as f64 * upp,
    );
    let [dr, db, dc] = [0, 1, 2].map(|i| d_strings(&svgs[i]).join(" "));
    let sw = vw / 300.0;
    let overlay = format!(
        r##"<svg viewBox="{vx:.3} {vy:.3} {vw:.3} {vw:.3}" xmlns="http://www.w3.org/2000/svg"><path d="{dr}" fill="#000" fill-opacity="0.08"/><path d="{dr}" fill="none" stroke="#cf222e" stroke-width="{:.4}"/><path d="{dc}" fill="none" stroke="#0969da" stroke-width="{:.4}" stroke-dasharray="{:.4} {:.4}"/></svg>"##,
        sw * 2.2,
        sw * 1.4,
        sw * 6.0,
        sw * 3.0,
    );
    let overview = format!(
        r##"<svg viewBox="{vbx} {vby} {vbh} {vbh}" xmlns="http://www.w3.org/2000/svg"><path d="{dc}" fill="#1f2328"/><rect x="{vx:.2}" y="{vy:.2}" width="{vw:.2}" height="{vw:.2}" fill="#d4a72c" fill-opacity="0.25" stroke="#bf8700" stroke-width="{:.2}"/></svg>"##,
        vbh / 190.0
    );
    let count = |d: &str, cs: &[char]| d.chars().filter(|c| cs.contains(c)).count();
    let [lr, lb, lc] = ctx.labels.each_ref().map(|s| esc(s));
    let q = r.q.as_ref().expect("examples are sampled");

    let mut s = String::new();
    let _ = write!(
        s,
        r##"<h2>Biggest byte saver, zoomed in</h2>
<p><b>{name}</b> <span class="muted">· {font} · {loc} · gid {gid}</span><br>
SVG size <code>{lb}</code> {b} B → <code>{lc}</code> <b>{c} B</b>: {d} ({saved} B).
The baseline has {nq} quadratic/smooth curve commands; the candidate has <b>{na} arcs</b>.</p>
<div class="zoomwide"><div>{overview}<br>whole icon (candidate) · highlighted = zoom window (1/16 of the icon)</div>
<div>{overlay}<br>vector overlay: <span style="color:#cf222e">━ {lr}</span> vs <span style="color:#0969da">┅ {lc}</span></div></div>
<div class="zoom">
<div><img src="{ir}"><br>{lr} @ {zpx}px-equivalent</div>
<div><img src="{ib}"><br>{lb}</div>
<div><img src="{ic}"><br>{lc}</div>
<div><img src="{ih}"><br>diff (red lost · blue added)</div></div>
<p class="muted">Zoomed window: MAE {zmae:.2}/255 · worst pixel Δ {zmax} · {zch:.1}% of pixels visibly changed · IoU {ziou:.4}. At normal sizes the candidate MAE is {m0:.2} @{s0}px and {m1:.2} @{s1}px.</p>
<details><summary>path data</summary><b>{lb}</b><pre>{dbe}</pre><b>{lc}</b><pre>{dce}</pre></details>"##,
        name = esc(&r.name),
        font = esc(&f.label),
        loc = esc(&f.locs[r.loc].label),
        gid = r.gid,
        b = r.bytes[1],
        c = r.bytes[2],
        d = delta(pct(r.bytes[1] as f64, r.bytes[2] as f64)),
        saved = r.bytes[1] as i64 - r.bytes[2] as i64,
        nq = count(&db, &['Q', 'q', 'T', 't']),
        na = count(&dc, &['A', 'a']),
        zpx = icon_px as u32,
        ir = png_data_uri(&zr),
        ib = png_data_uri(&zb),
        ic = png_data_uri(&zc),
        ih = png_data_uri(&heatmap(&zr, &zc)),
        zmae = zq.mae,
        zmax = zq.max,
        zch = zq.changed * 100.0,
        ziou = zq.iou,
        m0 = q.by_size[0][1].mae,
        s0 = ctx.sizes[0],
        m1 = q.by_size[q.by_size.len() - 1][1].mae,
        s1 = ctx.sizes[ctx.sizes.len() - 1],
        dbe = esc(&db),
        dce = esc(&dc),
    );
    Ok(s)
}

/// Aggregates as JSON, for tracking results over time.
pub fn json(ctx: &Ctx, fonts: &[FontData], recs: &[Rec]) -> String {
    let jstr = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
    let mut j = format!(
        "{{\"styles\":{{\"reference\":{},\"baseline\":{},\"candidate\":{}}},\"fonts\":[",
        jstr(&ctx.labels[0]),
        jstr(&ctx.labels[1]),
        jstr(&ctx.labels[2])
    );
    for (fi, f) in fonts.iter().enumerate() {
        if fi > 0 {
            j.push(',');
        }
        let _ = write!(
            j,
            "{{\"label\":{},\"glyphs\":{},\"locations\":[",
            jstr(&f.label),
            f.glyph_count
        );
        for (li, loc) in f.locs.iter().enumerate() {
            if li > 0 {
                j.push(',');
            }
            let rs: Vec<&Rec> = recs
                .iter()
                .filter(|r| r.font == fi && r.loc == li)
                .collect();
            let sum = |i: usize| rs.iter().map(|r| r.bytes[i]).sum::<usize>();
            let _ = write!(
                j,
                "{{\"label\":{},\"icons\":{},\"svg_bytes\":{{\"reference\":{},\"baseline\":{},\"candidate\":{}}},\"quality\":[",
                jstr(&loc.label),
                rs.len(),
                sum(0),
                sum(1),
                sum(2)
            );
            let qs: Vec<&QualRec> = rs.iter().filter_map(|r| r.q.as_deref()).collect();
            for (si, &size) in ctx.sizes.iter().enumerate() {
                if si > 0 {
                    j.push(',');
                }
                let m =
                    |k: usize| mean(&qs.iter().map(|q| q.by_size[si][k].mae).collect::<Vec<_>>());
                let _ = write!(
                    j,
                    "{{\"size\":{size},\"n\":{},\"mae_baseline\":{:.5},\"mae_candidate\":{:.5}}}",
                    qs.len(),
                    m(0),
                    m(1)
                );
            }
            j.push_str("]}");
        }
        let _ = write!(j, "],\"ablation_base\":{},\"ablation\":{{", f.ablation_base);
        for (i, (n, v)) in f.ablation.iter().enumerate() {
            if i > 0 {
                j.push(',');
            }
            let _ = write!(j, "\"{n}\":{v}");
        }
        j.push_str("}}");
    }
    j.push_str("]}");
    j.replace("NaN", "null")
}
