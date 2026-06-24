//! A command-line tool that converts all glyphs in a font file to individual SVG files.
use anyhow::{Context, Result};
use clap::Parser;
use rayon::prelude::*;
use skrifa::prelude::NormalizedCoord;
use skrifa::{instance::LocationRef, FontRef, MetadataProvider};
use sleipnir::{
    draw_icon::{DrawIcon, DrawOptions, DrawType},
    iconid::IconIdentifier,
    pathstyle::SvgPathStyle,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Parser)]
struct Args {
    /// Path to the ttf/otf font file
    #[arg(short, long)]
    font: PathBuf,

    /// Path to output html file.
    #[arg(short, long, default_value = "/tmp/sleipnir.html")]
    output_path: PathBuf,

    /// Icon size in pixels
    #[arg(short, long, default_value_t = 64.0)]
    size: f32,

    /// Variational design space coordinates
    #[arg(short, long, value_parser = parse_coords)]
    coords: Option<Vec<f32>>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    println!("Font file: {:?}", args.font);

    let data = fs::read(&args.font)
        .with_context(|| format!("Failed to read font file: {:?}", args.font))?;

    let font = FontRef::new(&data)
        .with_context(|| format!("Failed to parse font file: {:?}", args.font))?;

    let font_name = font
        .localized_strings(skrifa::string::StringId::FAMILY_NAME)
        .english_or_first()
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            args.font
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Unknown Font".to_string())
        });

    let glyph_names: Vec<_> = font.glyph_names().iter().collect();
    if glyph_names.is_empty() {
        panic!("No glyphs found in font.");
    }

    let svg_dir = svg_output_dir(&args.output_path);
    fs::create_dir_all(&svg_dir)
        .with_context(|| format!("Failed to create SVG output directory: {:?}", svg_dir))?;

    let coords = args.normalized_coords();
    let total_glyphs = glyph_names.len();
    let pad_width = total_glyphs.to_string().len();
    let processed_count = AtomicUsize::new(0);
    let results: Vec<_> = glyph_names
        .into_par_iter()
        // Write glyph to file
        .map(|(gid, name)| -> Result<Glyph> {
            let options = DrawOptions::new(
                IconIdentifier::GlyphId(gid),
                args.size,
                LocationRef::new(&coords),
                SvgPathStyle::Compact(2),
                DrawType::Svg,
            );
            let svg = font
                .draw_icon(&options)
                .map_err(|e| anyhow::anyhow!("Failed to draw icon for glyph {}: {:?}", name, e))?;
            let filename = format!(
                "{:0width$}_{}.svg",
                gid.to_u32(),
                sanitize_filename(name.as_str()),
                width = pad_width
            );
            let svg_path = svg_dir.join(&filename);
            fs::write(&svg_path, &svg)
                .with_context(|| format!("Failed to write SVG file: {:?}", svg_path))?;
            Ok(Glyph {
                glyph_name: name.to_string(),
                filename,
            })
        })
        // Progress accounting
        .inspect(|item_or_err| {
            let current = processed_count.fetch_add(1, Ordering::Relaxed) + 1;
            if current.is_multiple_of(1000) || current == total_glyphs {
                println!("Processed {}/{} glyphs...", current, total_glyphs);
            }
            if let Err(err) = item_or_err {
                eprintln!("{:?}", err);
            }
        })
        .collect();

    let (mut items, mut errors) = (Vec::new(), Vec::new());
    for res in results {
        match res {
            Ok(item) => items.push(item),
            Err(err) => errors.push(err),
        }
    }

    println!(
        "Processed {} glyphs successfully with {} errors",
        items.len(),
        errors.len()
    );
    println!("Wrote {} SVG files to {:?}", items.len(), svg_dir);

    // Generate HTML
    write_html_output(&args.output_path, &svg_dir, &font_name, &items, &errors)?;
    println!("Wrote HTML output to {:?}", &args.output_path);

    Ok(())
}

struct Glyph {
    glyph_name: String,
    filename: String,
}

fn svg_output_dir(output_path: &Path) -> PathBuf {
    let stem = output_path.file_stem().unwrap_or_else(|| "svgs".as_ref());
    let output_dir = output_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    output_dir.join(stem)
}

fn write_html_output(
    output_path: &Path,
    svg_dir: &Path,
    font_name: &str,
    items: &[Glyph],
    errors: &[anyhow::Error],
) -> Result<()> {
    let svg_dir_name = svg_dir
        .file_name()
        .map(|s| s.to_string_lossy())
        .unwrap_or_else(|| "svgs".into());

    let mut html = String::with_capacity(items.len() * 250 + 2000);
    html.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n");
    html.push_str("  <meta charset=\"UTF-8\">\n");
    html.push_str(&format!(
        "  <title>Font: {}</title>\n",
        html_escape(font_name)
    ));
    html.push_str("  <style>\n");
    html.push_str(include_str!("tohtml.css"));
    html.push_str("  </style>\n");
    html.push_str("</head>\n<body>\n");
    html.push_str(&format!("  <h1>Font: {}</h1>\n", html_escape(font_name)));
    html.push_str(&format!(
        "  <p>Total glyphs: {} (Success: {}, Errors: {})</p>\n",
        items.len() + errors.len(),
        items.len(),
        errors.len()
    ));

    html.push_str("  <div class=\"glyph-grid\">\n");
    for item in items {
        html.push_str(&render_glyph_card(item, &svg_dir_name));
    }
    html.push_str("  </div>\n");

    if !errors.is_empty() {
        html.push_str("  <div class=\"errors-section\">\n");
        html.push_str(&format!(
            "    <h2 class=\"errors-title\">Errors ({})</h2>\n",
            errors.len()
        ));
        html.push_str("    <pre class=\"error-log\">\n");
        for err in errors {
            html.push_str(&format!("{:?}\n", err));
        }
        html.push_str("    </pre>\n  </div>\n");
    }
    html.push_str("</body>\n</html>\n");

    fs::write(output_path, html)
        .with_context(|| format!("Failed to write HTML to {:?}", output_path))?;

    Ok(())
}

impl Args {
    fn normalized_coords(&self) -> Vec<NormalizedCoord> {
        match self.coords.as_ref() {
            Some(c) => c.iter().copied().map(NormalizedCoord::from_f32).collect(),
            None => Vec::new(),
        }
    }
}

fn parse_coords(s: &str) -> Result<Vec<f32>, String> {
    s.split(',')
        .map(|p| {
            p.parse::<f32>()
                .map_err(|e| format!("Failed to parse coordinate value '{}': {}", p, e))
        })
        .collect()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

fn sanitize_filename(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        "glyph".to_string()
    } else {
        s
    }
}

fn render_glyph_card(item: &Glyph, svg_dir_name: &str) -> String {
    let rel_svg_path = format!("{}/{}", svg_dir_name, item.filename);
    format!(
        r#"<div class="glyph-card">
  <div class="glyph-svg">
    <img src="{}" alt="{}" loading="lazy" />
  </div>
  <div class="glyph-name">{}</div>
</div>
"#,
        html_escape(&rel_svg_path),
        html_escape(&item.glyph_name),
        html_escape(&item.glyph_name)
    )
}
