//! A command-line tool that converts all glyphs in a font file to individual SVG files.
use anyhow::{Context, Result};
use clap::Parser;
use rayon::prelude::*;
use skrifa::prelude::NormalizedCoord;
use skrifa::{instance::LocationRef, FontRef, MetadataProvider};
use sleipnir::{
    draw_glyph::DrawOptions, icon2svg::draw_icon, iconid::IconIdentifier, pathstyle::SvgPathStyle,
};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Parser)]
struct Args {
    /// Path to the ttf/otf font file
    #[arg(short, long)]
    font: PathBuf,

    /// Directory to output the SVG files
    #[arg(short, long, default_value = "/tmp/sleipnir-svg")]
    output_dir: PathBuf,

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

    let glyph_names: Vec<_> = font.glyph_names().iter().collect();
    if glyph_names.is_empty() {
        println!("No glyphs found in font.");
        return Ok(());
    }

    let coords = args.normalized_coords();
    let total_glyphs = glyph_names.len();
    let processed_count = AtomicUsize::new(0);
    fs::create_dir_all(&args.output_dir)
        .with_context(|| format!("Failed to create output directory: {:?}", args.output_dir))?;
    let errors: Vec<_> = glyph_names
        .into_par_iter()
        .map(|(gid, name)| -> Result<()> {
            let options = DrawOptions::new(
                IconIdentifier::GlyphId(gid),
                args.size,
                LocationRef::new(&coords),
                SvgPathStyle::Compact(2),
            );
            let svg = draw_icon(&font, &options)
                .map_err(|e| anyhow::anyhow!("Failed to draw icon for glyph {}: {:?}", name, e))?;
            let output_path = args.output_dir.join(format!("{}.svg", name));
            fs::write(&output_path, svg)
                .with_context(|| format!("Failed to write SVG to {:?}", output_path))?;

            Ok(())
        })
        .inspect(|_| {
            let current = processed_count.fetch_add(1, Ordering::Relaxed) + 1;
            if current.is_multiple_of(1000) || current == total_glyphs {
                println!("Processed {}/{} glyphs...", current, total_glyphs);
            }
        })
        .filter_map(Result::err)
        .collect();

    for err in errors.iter() {
        eprintln!("{:?}", err);
    }
    println!(
        "Wrote {} glyphs to {:?}",
        total_glyphs - errors.len(),
        args.output_dir
    );
    if !errors.is_empty() {
        anyhow::bail!("Failed to process {} glyphs", errors.len());
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
