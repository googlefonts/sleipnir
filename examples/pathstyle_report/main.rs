//! Measures how an [`SvgPathStyle`] change affects SVG size and rendering quality, and writes a
//! self-contained HTML report.
//!
//! For every glyph in each font, at a set of variable-font locations, the icon is drawn with
//! three styles:
//!
//! * `--reference` (default `unchanged`): the quality ground truth.
//! * `--baseline` (default `compact`): the style in use today.
//! * `--candidate` (default `minimized`): the proposed style.
//!
//! Sizes are full SVG bytes. Quality is measured by rasterizing with tiny-skia at several pixel
//! sizes and comparing alpha masks against the reference.
//!
//! ```shell
//! # Bundled test font, default locations (each axis min/max plus combinations).
//! cargo run --release --example pathstyle_report
//!
//! # Several fonts, explicit locations, rasterize every glyph.
//! cargo run --release --example pathstyle_report -- \
//!     --font a.ttf --font b.ttf \
//!     --var opsz,wght,FILL,GRAD,ROND@48,100,0,-25,100 --var wght@700 --full
//! ```
mod raster;
mod report;

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use raster::{alpha, compare, d_strings, mae, Shape, Q};
use rayon::prelude::*;
use skrifa::{instance::LocationRef, prelude::NormalizedCoord, FontRef, GlyphId, MetadataProvider};
use sleipnir::{
    draw_icon::{DrawIcon, DrawOptions, DrawType},
    iconid::IconIdentifier,
    pathstyle::{MakeArcsConfig, SvgPathStyle},
};
use std::path::{Path, PathBuf};
use std::time::Instant;

const DEFAULT_FONT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resources/testdata/MaterialSymbolsOutlinedVF-Popular.ttf"
);
const DEFAULT_OUTPUT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/target/pathstyle_report/report.html"
);

/// Named path styles that can be compared.
#[derive(Copy, Clone, Debug, PartialEq, ValueEnum)]
enum StyleArg {
    /// `SvgPathStyle::unchanged`.
    Unchanged,
    /// `SvgPathStyle::compact`.
    Compact,
    /// `SvgPathStyle::minimized` (lossy arc fitting).
    Minimized,
    /// `SvgPathStyle::minimized` with `MakeArcsConfig::svgo()` arc tolerances.
    MinimizedSvgo,
}

impl StyleArg {
    fn style(self, precision: usize) -> SvgPathStyle {
        match self {
            Self::Unchanged => SvgPathStyle::unchanged(precision),
            Self::Compact => SvgPathStyle::compact(precision),
            Self::Minimized => SvgPathStyle::minimized(precision),
            Self::MinimizedSvgo => {
                SvgPathStyle::minimized(precision).with_make_arcs(Some(MakeArcsConfig::svgo()))
            }
        }
    }

    fn label(self, precision: usize) -> String {
        let name = self
            .to_possible_value()
            .map(|v| v.get_name().to_string())
            .unwrap_or_default();
        format!("{name}({precision})")
    }
}

/// A variable-font location given on the command line as `TAG,TAG@VALUE,VALUE`.
#[derive(Clone, Debug)]
struct VarLoc(Vec<(String, f32)>);

fn parse_var(s: &str) -> Result<VarLoc, String> {
    let (tags, values) = s
        .split_once('@')
        .ok_or_else(|| format!("expected TAG,TAG@VALUE,VALUE, got {s:?}"))?;
    let tags: Vec<&str> = tags.split(',').map(str::trim).collect();
    let values: Vec<f32> = values
        .split(',')
        .map(|v| {
            v.trim()
                .parse()
                .map_err(|e| format!("bad value {v:?}: {e}"))
        })
        .collect::<Result<_, _>>()?;
    if tags.len() != values.len() {
        return Err(format!(
            "{} axis tags but {} values in {s:?}",
            tags.len(),
            values.len()
        ));
    }
    if let Some(t) = tags.iter().find(|t| t.is_empty() || t.len() > 4) {
        return Err(format!("invalid axis tag {t:?}"));
    }
    Ok(VarLoc(
        tags.into_iter().map(String::from).zip(values).collect(),
    ))
}

#[derive(Parser)]
#[command(about = "Compare SVG path styles for size and rendering quality")]
struct Args {
    /// Font file to analyze. Repeat for several fonts. Defaults to the bundled Material Symbols
    /// test font.
    #[arg(short, long = "font")]
    fonts: Vec<PathBuf>,

    /// Variable-font location as `TAG,TAG@VALUE,VALUE` (e.g. `opsz,wght@48,100`). Repeatable.
    /// Axes a font lacks are ignored. When omitted: the default location, each axis at its min
    /// and max, and all axes at min, max and midpoint.
    #[arg(long = "var", value_parser = parse_var)]
    vars: Vec<VarLoc>,

    /// Style used as quality ground truth.
    #[arg(long, value_enum, default_value_t = StyleArg::Unchanged)]
    reference: StyleArg,

    /// Style in use today.
    #[arg(long, value_enum, default_value_t = StyleArg::Compact)]
    baseline: StyleArg,

    /// Proposed style.
    #[arg(long, value_enum, default_value_t = StyleArg::Minimized)]
    candidate: StyleArg,

    /// Decimal precision for all styles.
    #[arg(long, default_value_t = 2)]
    precision: usize,

    /// Raster sizes in pixels.
    #[arg(long, value_delimiter = ',', default_value = "24,64,128,256")]
    sizes: Vec<u32>,

    /// Approximate glyphs per font and location to rasterize. Size metrics always use every glyph.
    #[arg(long, default_value_t = 1500)]
    sample: usize,

    /// Rasterize every glyph.
    #[arg(long)]
    full: bool,

    /// Only consider the first N glyphs of each font (for quick runs).
    #[arg(long)]
    max_glyphs: Option<usize>,

    /// Output HTML path. A `.json` file with aggregates is written alongside.
    #[arg(short, long, default_value = DEFAULT_OUTPUT)]
    output: PathBuf,
}

/// The styles being compared, indexed `[reference, baseline, candidate]`.
pub struct Ctx {
    pub styles: [SvgPathStyle; 3],
    pub labels: [String; 3],
    /// High precision unchanged style used to expose rounding error.
    pub hi_ref: SvgPathStyle,
    pub hi_ref_label: String,
    pub sizes: Vec<u32>,
    pub sampling: String,
}

pub struct Location {
    pub label: String,
    pub coords: Vec<NormalizedCoord>,
}

pub struct QualRec {
    /// Per size: `[baseline, candidate]` compared to the reference.
    pub by_size: Vec<[Q; 2]>,
    /// MAE at the smallest size vs the high precision reference, for
    /// `[reference, baseline, candidate]`.
    pub mae_hi: [f64; 3],
}

pub struct Rec {
    pub font: usize,
    pub loc: usize,
    pub gid: u32,
    pub name: String,
    /// Full SVG bytes for `[reference, baseline, candidate]`.
    pub bytes: [usize; 3],
    /// Number of arc commands in the candidate output.
    pub arcs: usize,
    pub q: Option<Box<QualRec>>,
}

pub struct FontData {
    pub label: String,
    pub data: Vec<u8>,
    pub glyph_count: usize,
    pub locs: Vec<Location>,
    pub empty: usize,
    pub errors: usize,
    /// Baseline bytes at the default location (all glyphs).
    pub ablation_base: usize,
    /// Bytes with a single option switched from baseline to candidate.
    pub ablation: Vec<(&'static str, usize)>,
}

pub fn draw(
    font: &FontRef,
    gid: u32,
    coords: &[NormalizedCoord],
    style: SvgPathStyle,
) -> Result<String> {
    let opts = DrawOptions::new(
        IconIdentifier::GlyphId(GlyphId::new(gid)),
        24.0,
        LocationRef::new(coords),
        style,
        DrawType::Svg,
    );
    font.draw_icon(&opts)
        .map_err(|e| anyhow::anyhow!("gid {gid}: {e:?}"))
}

fn has_path_data(svg: &str) -> bool {
    d_strings(svg).iter().any(|d| !d.is_empty())
}

/// Each option that differs between `base` and `cand`, applied individually to `base`.
fn ablations(base: SvgPathStyle, cand: SvgPathStyle) -> Vec<(&'static str, SvgPathStyle)> {
    let mut out = Vec::new();
    macro_rules! toggle {
        ($($field:ident),*) => {$(
            if base.$field != cand.$field {
                let mut s = base;
                s.$field = cand.$field;
                out.push((stringify!($field), s));
            }
        )*};
    }
    toggle!(
        precision,
        utilize_absolute,
        line_shorthands,
        curve_smooth_shorthands,
        remove_useless,
        convert_to_z,
        collapse_collinear,
        omit_repeated_commands,
        leading_zero,
        make_arcs
    );
    out
}

fn fmt_num(v: f32) -> String {
    format!("{}", (v * 100.0).round() / 100.0)
}

fn settings_label(settings: &[(String, f32)]) -> String {
    settings
        .iter()
        .map(|(t, v)| format!("{t}={}", fmt_num(*v)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Axis tag and user-space value pairs.
type Settings = Vec<(String, f32)>;

/// Resolves the locations to test for `font`.
fn font_locations(font: &FontRef, vars: &[VarLoc]) -> Vec<Location> {
    let axes: Vec<(String, f32, f32, f32)> = font
        .axes()
        .iter()
        .map(|a| {
            (
                a.tag().to_string(),
                a.min_value(),
                a.default_value(),
                a.max_value(),
            )
        })
        .collect();
    let mut candidates: Vec<(Option<&str>, Settings)> = vec![(Some("default"), vec![])];
    if vars.is_empty() {
        for (tag, min, def, max) in &axes {
            for v in [*min, *max] {
                if v != *def {
                    candidates.push((None, vec![(tag.clone(), v)]));
                }
            }
        }
        if axes.len() > 1 {
            let all = |f: &dyn Fn(f32, f32) -> f32| {
                axes.iter()
                    .map(|(t, min, _, max)| (t.clone(), f(*min, *max)))
                    .collect()
            };
            candidates.push((Some("all min"), all(&|min, _| min)));
            candidates.push((Some("all max"), all(&|_, max| max)));
            candidates.push((Some("all mid"), all(&|min, max| (min + max) / 2.0)));
        }
    } else {
        candidates.extend(vars.iter().map(|v| (None, v.0.clone())));
    }

    let mut out: Vec<Location> = Vec::new();
    for (name, settings) in candidates {
        let present: Vec<(String, f32)> = settings
            .iter()
            .filter(|(t, _)| axes.iter().any(|a| &a.0 == t))
            .cloned()
            .collect();
        if present.is_empty() && !settings.is_empty() {
            continue;
        }
        let location = font
            .axes()
            .location(present.iter().map(|(t, v)| (t.as_str(), *v)));
        let coords = location.coords().to_vec();
        if out.iter().any(|l| l.coords == coords) {
            continue;
        }
        let label = match name {
            Some(n) if present.len() > 1 => format!("{n} ({})", settings_label(&present)),
            Some(n) => n.to_string(),
            None => settings_label(&present),
        };
        out.push(Location { label, coords });
    }
    out
}

fn font_label(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn main() -> Result<()> {
    let mut args = Args::parse();
    if args.fonts.is_empty() {
        args.fonts.push(PathBuf::from(DEFAULT_FONT));
    }
    args.sizes.sort_unstable();
    args.sizes.dedup();
    anyhow::ensure!(
        !args.sizes.is_empty() && args.sizes[0] > 0,
        "--sizes must contain positive values"
    );

    let p = args.precision;
    let ctx = Ctx {
        styles: [
            args.reference.style(p),
            args.baseline.style(p),
            args.candidate.style(p),
        ],
        labels: [
            args.reference.label(p),
            args.baseline.label(p),
            args.candidate.label(p),
        ],
        hi_ref: SvgPathStyle::unchanged(6),
        hi_ref_label: StyleArg::Unchanged.label(6),
        sizes: args.sizes.clone(),
        sampling: if args.full {
            "all glyphs rasterized".to_string()
        } else {
            format!("~{} glyphs per font and location rasterized", args.sample)
        },
    };
    let start = Instant::now();
    let mut fonts = Vec::new();
    let mut recs = Vec::new();

    for (fi, path) in args.fonts.iter().enumerate() {
        let data = std::fs::read(path).with_context(|| format!("reading {path:?}"))?;
        let mut fd = collect_font(fi, path, &data, &args, &ctx, &mut recs, start)?;
        fd.data = data;
        fonts.push(fd);
    }

    let html = report::build(&ctx, &fonts, &recs, start.elapsed().as_secs_f64())?;
    if let Some(dir) = args.output.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {dir:?}"))?;
    }
    std::fs::write(&args.output, html).with_context(|| format!("writing {:?}", args.output))?;
    std::fs::write(
        args.output.with_extension("json"),
        report::json(&ctx, &fonts, &recs),
    )?;
    println!(
        "[{:>6.1}s] wrote {}",
        start.elapsed().as_secs_f64(),
        args.output.display()
    );
    Ok(())
}

fn collect_font(
    fi: usize,
    path: &Path,
    data: &[u8],
    args: &Args,
    ctx: &Ctx,
    recs: &mut Vec<Rec>,
    start: Instant,
) -> Result<FontData> {
    let font = FontRef::new(data).with_context(|| format!("parsing {path:?}"))?;
    let label = font_label(path);
    let names: Vec<(u32, String)> = font
        .glyph_names()
        .iter()
        .map(|(g, n)| (g.to_u32(), n.to_string()))
        .filter(|(g, _)| *g != 0)
        .take(args.max_glyphs.unwrap_or(usize::MAX))
        .collect();
    let step = if args.full {
        1
    } else {
        (names.len() / args.sample.max(1)).max(1)
    };
    let mut fd = FontData {
        label: label.clone(),
        data: Vec::new(),
        glyph_count: names.len(),
        locs: font_locations(&font, &args.vars),
        empty: 0,
        errors: 0,
        ablation_base: 0,
        ablation: Vec::new(),
    };

    let [reference, baseline, candidate] = ctx.styles;
    let min_size = ctx.sizes[0];
    for (li, loc) in fd.locs.iter().enumerate() {
        let coords = &loc.coords;
        let results: Vec<Result<Option<Rec>>> = names
            .par_iter()
            .enumerate()
            .map(|(idx, (gid, name))| -> Result<Option<Rec>> {
                let gid = *gid;
                let svgs = [
                    draw(&font, gid, coords, reference)?,
                    draw(&font, gid, coords, baseline)?,
                    draw(&font, gid, coords, candidate)?,
                ];
                if !has_path_data(&svgs[0]) {
                    return Ok(None);
                }
                let arcs = d_strings(&svgs[2])
                    .iter()
                    .map(|d| d.chars().filter(|c| matches!(c, 'A' | 'a')).count())
                    .sum();
                let q = if idx % step == 0 {
                    let shapes = svgs
                        .iter()
                        .map(|s| Shape::from_svg(s))
                        .collect::<Result<Vec<_>>>()
                        .with_context(|| format!("gid {gid}"))?;
                    let by_size = ctx
                        .sizes
                        .iter()
                        .map(|&size| {
                            let [r, b, c] = [0, 1, 2].map(|i| alpha(&shapes[i].render(size)));
                            [compare(&r, &b), compare(&r, &c)]
                        })
                        .collect();
                    let hi = alpha(
                        &Shape::from_svg(&draw(&font, gid, coords, ctx.hi_ref)?)?.render(min_size),
                    );
                    let mae_hi = [0, 1, 2].map(|i| mae(&hi, &alpha(&shapes[i].render(min_size))));
                    Some(Box::new(QualRec { by_size, mae_hi }))
                } else {
                    None
                };
                Ok(Some(Rec {
                    font: fi,
                    loc: li,
                    gid,
                    name: name.clone(),
                    bytes: [svgs[0].len(), svgs[1].len(), svgs[2].len()],
                    arcs,
                    q,
                }))
            })
            .collect();
        let (mut ok, mut empty, mut errors) = (0, 0, 0);
        for r in results {
            match r {
                Ok(Some(r)) => {
                    ok += 1;
                    recs.push(r);
                }
                Ok(None) => empty += 1,
                Err(e) => {
                    if errors < 3 {
                        eprintln!("  error: {e:#}");
                    }
                    errors += 1;
                }
            }
        }
        if li == 0 {
            fd.empty = empty;
            fd.errors = errors;
        }
        println!(
            "[{:>6.1}s] {label} / {}: {ok} icons, {empty} empty, {errors} errors",
            start.elapsed().as_secs_f64(),
            loc.label
        );
    }

    // Single-option ablation at the default location.
    let coords0 = &fd.locs[0].coords;
    let abl = ablations(baseline, candidate);
    let totals = names
        .par_iter()
        .filter_map(|(gid, _)| {
            let base = draw(&font, *gid, coords0, baseline).ok()?;
            if !has_path_data(&base) {
                return None;
            }
            let mut v = vec![base.len()];
            for (_, style) in &abl {
                v.push(draw(&font, *gid, coords0, *style).ok()?.len());
            }
            Some(v)
        })
        .reduce(
            || vec![0; abl.len() + 1],
            |a, b| a.iter().zip(&b).map(|(x, y)| x + y).collect(),
        );
    fd.ablation_base = totals[0];
    fd.ablation = abl
        .iter()
        .zip(&totals[1..])
        .map(|((n, _), v)| (*n, *v))
        .collect();
    Ok(fd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_var_locations() {
        let v = parse_var("opsz,wght,FILL,GRAD,ROND@48,100,0,-25,100").unwrap();
        assert_eq!(v.0.len(), 5);
        assert_eq!(v.0[3], ("GRAD".to_string(), -25.0));
        assert!(parse_var("wght").is_err());
        assert!(parse_var("wght,opsz@100").is_err());
        assert!(parse_var("toolong@1").is_err());
    }

    #[test]
    fn ablations_only_cover_differing_options() {
        let names: Vec<_> = ablations(SvgPathStyle::compact(2), SvgPathStyle::minimized(2))
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        assert_eq!(
            names,
            [
                "convert_to_z",
                "collapse_collinear",
                "omit_repeated_commands",
                "leading_zero",
                "make_arcs"
            ]
        );
    }

    #[test]
    fn bundled_font_locations_are_unique() {
        let data = std::fs::read(DEFAULT_FONT).unwrap();
        let font = FontRef::new(&data).unwrap();
        let locs = font_locations(&font, &[]);
        assert_eq!(locs[0].label, "default");
        assert!(locs.len() > 1);
        for (i, a) in locs.iter().enumerate() {
            assert!(locs[i + 1..].iter().all(|b| b.coords != a.coords));
        }
    }
}
