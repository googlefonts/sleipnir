//! A command-line tool that generates an HTML file comparing native font rendering vs SVG rendering.
use anyhow::{Context, Result};
use clap::Parser;
use rayon::prelude::*;
use skrifa::prelude::NormalizedCoord;
use skrifa::{instance::LocationRef, FontRef, MetadataProvider};
use sleipnir::{
    draw_icon::{DrawIcon, DrawOptions, DrawType, ViewBoxMode},
    iconid::IconIdentifier,
    pathstyle::SvgPathStyle,
};
use std::fs;
use std::path::PathBuf;

#[derive(Parser)]
struct Args {
    /// Path to the ttf/otf font file
    #[arg(short, long)]
    font: PathBuf,

    /// Directory to output the HTML and font files
    #[arg(short, long, default_value = "/tmp/sleipnir-compare")]
    output_dir: PathBuf,

    /// Render size in pixels
    #[arg(short, long, default_value_t = 64.0)]
    size: f32,

    /// Variational design space coordinates
    #[arg(short, long, value_parser = parse_coords)]
    coords: Option<Vec<f32>>,

    /// Automatically open the generated HTML in a web browser
    #[arg(long)]
    open: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    println!("Font file: {:?}", args.font);

    let data = fs::read(&args.font)
        .with_context(|| format!("Failed to read font file: {:?}", args.font))?;

    let font = FontRef::new(&data)
        .with_context(|| format!("Failed to parse font file: {:?}", args.font))?;

    fs::create_dir_all(&args.output_dir)
        .with_context(|| format!("Failed to create output directory: {:?}", args.output_dir))?;

    // Copy font file to output directory
    let font_file_name = args.font.file_name().context("Invalid font path")?;
    let output_font_path = args.output_dir.join(font_file_name);
    fs::copy(&args.font, &output_font_path)
        .with_context(|| format!("Failed to copy font to {:?}", output_font_path))?;
    println!("Copied font to {:?}", output_font_path);

    let charmap = font.charmap();
    let glyph_names = font.glyph_names();

    let chars_to_process: Vec<char> = charmap
        .mappings()
        .filter_map(|(codepoint, _)| char::from_u32(codepoint))
        .collect();

    let coords = args.normalized_coords();
    let size = args.size;

    println!("Processing {} characters...", chars_to_process.len());

    let results: Vec<(char, String, Result<String, String>)> = chars_to_process
        .into_par_iter()
        .map(|ch| {
            let codepoint = ch as u32;
            let gid = match charmap.map(codepoint) {
                Some(gid) => gid,
                None => return (ch, "unknown".to_string(), Err("Not found in charmap".to_string())),
            };

            let name = glyph_names
                .get(gid)
                .map(|gn| gn.as_str().to_string())
                .unwrap_or_else(|| format!("gid_{}", gid.to_u32()));

            let mut options = DrawOptions::new(
                IconIdentifier::GlyphId(gid),
                size,
                LocationRef::new(&coords),
                SvgPathStyle::Compact(2),
                DrawType::Svg,
            );
            options.viewbox_mode = ViewBoxMode::UseBoundingBox;

            let svg_res = font
                .draw_icon(&options)
                .map_err(|e| format!("{:?}", e));

            (ch, name, svg_res)
        })
        .collect();

    let mut cards_html = String::new();
    let mut success_count = 0;
    let mut failure_count = 0;
    for (ch, name, svg_res) in results {
        let codepoint = ch as u32;
        let escaped_char = match ch {
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '&' => "&amp;".to_string(),
            '"' => "&quot;".to_string(),
            _ => ch.to_string(),
        };

        let svg_content = match svg_res {
            Ok(svg) => {
                success_count += 1;
                svg
            }
            Err(err_msg) => {
                failure_count += 1;
                format!(r#"<div class="error-box" title="{}">Error</div>"#, err_msg)
            }
        };

        cards_html.push_str(&format!(
            r#"
    <div class="card">
      <div class="label">U+{codepoint:04X} ({name})</div>
      <div class="renders">
        <div class="render-container">
          <div class="render-label">Font</div>
          <div class="render-box font-box">{char}</div>
        </div>
        <div class="render-container">
          <div class="render-label">SVG</div>
          <div class="render-box svg-box">{svg_content}</div>
        </div>
      </div>
    </div>
"#,
            codepoint = codepoint,
            name = name,
            char = escaped_char,
            svg_content = svg_content
        ));
    }

    let css = format!(
        r#"
@font-face {{
    font-family: 'CompareFont';
    src: url('{font_file_name}');
}}
body {{
    font-family: sans-serif;
    background: #f0f0f0;
    margin: 20px;
}}
.grid {{
    display: flex;
    flex-wrap: wrap;
    gap: 15px;
}}
.card {{
    border: 1px solid #ccc;
    border-radius: 8px;
    padding: 10px;
    display: flex;
    flex-direction: column;
    align-items: center;
    background: white;
    box-shadow: 0 2px 4px rgba(0,0,0,0.1);
}}
.label {{
    font-family: monospace;
    font-size: 11px;
    margin-bottom: 8px;
    color: #555;
    text-align: center;
}}
.renders {{
    display: flex;
    gap: 10px;
    border: 1px dashed #ccc;
    padding: 5px;
    background: #fafafa;
}}
.render-container {{
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
}}
.render-label {{
    font-family: monospace;
    font-size: 9px;
    color: #888;
    text-transform: uppercase;
}}
.render-box {{
    height: {size}px;
    min-width: {size}px;
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
}}
.font-box {{
    font-family: 'CompareFont';
    font-size: {size}px;
    line-height: 1;
    text-align: center;
    padding: 0 10px;
}}
.svg-box svg {{
    height: 100%;
    width: auto;
    border: 1px solid rgba(0, 0, 255, 0.3);
}}
.error-box {{
    color: red;
    font-size: 10px;
    font-family: sans-serif;
    border: 1px dashed red;
    padding: 2px;
    text-align: center;
    display: flex;
    align-items: center;
    justify-content: center;
    height: {size}px;
    width: {size}px;
    box-sizing: border-box;
    cursor: help;
}}
"#,
        font_file_name = font_file_name.to_string_lossy(),
        size = size
    );

    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>Font vs SVG Comparison</title>
  <style>
    {css}
  </style>
</head>
<body>
  <h1>Font vs SVG Comparison: {font_name}</h1>
  <p>Comparison of native font rendering (Font) vs SVG rendering (SVG).</p>
  <p>Successes: {success_count} | Failures: {failure_count}</p>
  <div class="grid">
    {cards}
  </div>
</body>
</html>
"#,
        css = css,
        font_name = font_file_name.to_string_lossy(),
        success_count = success_count,
        failure_count = failure_count,
        cards = cards_html
    );

    let output_html_path = args.output_dir.join("compare.html");
    fs::write(&output_html_path, html)
        .with_context(|| format!("Failed to write HTML to {:?}", output_html_path))?;

    println!(
        "Successfully wrote {} glyph comparisons ({} successes, {} failures) to {:?}",
        success_count + failure_count, success_count, failure_count, output_html_path
    );

    if args.open {
        println!("Opening in browser...");
        open_in_browser(&output_html_path)?;
    }

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

fn open_in_browser(path: &std::path::Path) -> Result<()> {
    let result = if cfg!(target_os = "windows") {
        std::process::Command::new("cmd")
            .args(["/C", "start", &path.to_string_lossy()])
            .spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
    } else {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
    };

    result.context("Failed to open browser")?;
    Ok(())
}
