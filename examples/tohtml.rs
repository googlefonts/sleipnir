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
use std::path::PathBuf;
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

struct Item {
    glyph_name: String,
    svg: String,
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

    let coords = args.normalized_coords();
    let total_glyphs = glyph_names.len();
    let processed_count = AtomicUsize::new(0);
    let results: Vec<_> = glyph_names
        .into_par_iter()
        .map(|(gid, name)| -> Result<Item> {
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
            Ok(Item {
                glyph_name: name.to_string(),
                svg,
            })
        })
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

    // Generate HTML
    write_html_output(&args.output_path, &font_name, &items, &errors)?;

    Ok(())
}

const HTML_STYLE: &str = include_str!("tohtml.css");

fn write_html_output(
    output_path: &std::path::Path,
    font_name: &str,
    items: &[Item],
    errors: &[anyhow::Error],
) -> Result<()> {
    let mut html = String::new();
    html.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n");
    html.push_str("  <meta charset=\"UTF-8\">\n");
    html.push_str(&format!(
        "  <title>Font: {}</title>\n",
        html_escape(font_name)
    ));
    html.push_str("  <style>\n");
    html.push_str(HTML_STYLE);
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
        html.push_str(&render_glyph_card(item));
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

    println!("Wrote HTML output to {:?}", output_path);

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

fn render_glyph_card(item: &Item) -> String {
    format!(
        "    <div class=\"glyph-card\">\n      <div class=\"glyph-svg\">\n{}\n      </div>\n      <div class=\"glyph-name\">{}</div>\n    </div>\n",
        item.svg,
        html_escape(&item.glyph_name)
    )
}
