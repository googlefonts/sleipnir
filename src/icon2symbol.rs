//! Produces Apple Symbols from SVGs.

use crate::error::DrawSvgError;
use crate::pathstyle::SvgPathStyle;
use kurbo::{Affine, BezPath};
use regex::Regex;
use roxmltree::Document;

const SYMBOL_BASE_SIZE: f64 = 120.0;
const CENTER_LINE: f64 = -35.23;

pub fn draw_apple_symbols(layer_svgs: Vec<(&str, &str)>) -> Result<String, DrawSvgError> {
    let template_svg = include_str!("../resources/symbol_template.svg");

    let mut modified_svg = template_svg.to_string();

    // Remove the XML declaration if present
    if modified_svg.starts_with("<?xml") {
        if let Some(index) = modified_svg.find("?>") {
            modified_svg = modified_svg[index + 2..].trim_start().to_string();
        }
    }

    for (layer_name, svg_content) in layer_svgs {
        let (path_d, src) = extract_svg_details(svg_content)?;

        let mut bez_path =
            BezPath::from_svg(&path_d).map_err(|e| DrawSvgError::ParseError(e.to_string()))?;

        let transform = build_transformation(layer_name, src);

        bez_path.apply_affine(transform);

        let transformed_path_d = SvgPathStyle::Rounding(3).write_svg_path(&bez_path);

        // This is where you would use an XML writing library to find the group and insert the path.
        // For example, find <g id="Regular-M"> and add <path d="..."/> as a child.
        // Since we don't have a great XML writer, we'll just do a string replace on the empty group.
        let group_tag_regex = format!(r#"<g (id="{}"[^>]*)>\s*</g>"#, layer_name);
        let re = Regex::new(&group_tag_regex).unwrap();
        let replacement = format!("<g $1><path d=\"{}\"/></g>", transformed_path_d);

        if re.is_match(&modified_svg) {
            modified_svg = re.replace(&modified_svg, replacement).to_string();
        } else {
            // Handle cases where the group might already have content or a different structure if needed.
            eprintln!(
                "Warning: Group tag for {} not found or not empty.",
                layer_name
            );
        }
    }

    // Remove empty groups
    let empty_group_regex = Regex::new(r#"<g id="[^"]*" transform="[^"]*"></g>\s*"#).unwrap();
    let cleaned_svg = empty_group_regex.replace_all(&modified_svg, "").to_string();

    Ok(cleaned_svg)
}

struct ViewBox {
    pub x: f64,
    // min-y
    pub y: f64,
    // width
    pub w: f64,
    // height
    pub h: f64,
}

// Helper to extract path data from an SVG element, ensuring it only contains one path.
fn extract_path_d(svg_element: &roxmltree::Node) -> Result<String, DrawSvgError> {
    let mut elements = svg_element
        .children()
        .filter(|n| n.is_element() && !n.has_tag_name("defs") && !n.has_tag_name("title"));

    let path_node = elements
        .next()
        .ok_or_else(|| DrawSvgError::InvalidSvg("No elements found in SVG".to_string()))?;

    if !path_node.has_tag_name("path") || elements.next().is_some() {
        return Err(DrawSvgError::InvalidSvg(
            "SVG must contain exactly one path element".to_string(),
        ));
    }

    path_node
        .attribute("d")
        .map(|s| s.to_string())
        .ok_or_else(|| DrawSvgError::InvalidSvg("Path element missing d attribute".to_string()))
}

// Helper to extract path data and viewbox from a simple SVG string
fn extract_svg_details(svg_content: &str) -> Result<(String, ViewBox), DrawSvgError> {
    let doc = Document::parse(svg_content).map_err(|e| DrawSvgError::ParseError(e.to_string()))?;
    let svg_element = doc
        .root_element()
        .children()
        .find(|n| n.has_tag_name("svg"))
        .unwrap_or(doc.root_element());

    let path_d = extract_path_d(&svg_element)?;

    let viewbox_str = svg_element.attribute("viewBox");
    let width_str = svg_element.attribute("width");
    let height_str = svg_element.attribute("height");

    let rect = if let Some(vb) = viewbox_str {
        let parts: Vec<Result<f64, _>> = vb.split(' ').map(|s| s.parse()).collect();
        if parts.len() == 4 && parts.iter().all(|p| p.is_ok()) {
            let nums: Vec<f64> = parts.into_iter().map(|p| p.unwrap()).collect();
            ViewBox {
                x: nums[0],
                y: nums[1],
                w: nums[2],
                h: nums[3],
            }
        } else {
            return Err(DrawSvgError::InvalidSvg(format!("Invalid viewBox: {vb}")));
        }
    } else if let (Some(w), Some(h)) = (width_str, height_str) {
        let width: f64 = w
            .parse()
            .map_err(|_| DrawSvgError::InvalidSvg(format!("Invalid width: {w}")))?;
        let height: f64 = h
            .parse()
            .map_err(|_| DrawSvgError::InvalidSvg(format!("Invalid height: {h}")))?;
        ViewBox {
            x: 0.0,
            y: 0.0,
            w: width,
            h: height,
        }
    } else {
        return Err(DrawSvgError::InvalidSvg(
            "SVG must have a viewBox or width/height".to_string(),
        ));
    };
    Ok((path_d, rect))
}

fn get_symbol_scale(symbol_name: &str) -> f64 {
    match symbol_name.chars().last() {
        Some('S') => 0.789,
        Some('M') => 1.0,
        Some('L') => 1.29,
        _ => 1.0, // Default to Medium scale
    }
}

fn symbol_size(symbol_name: &str) -> f64 {
    get_symbol_scale(symbol_name) * SYMBOL_BASE_SIZE
}

fn build_transformation(symbol_name: &str, src: ViewBox) -> Affine {
    let size = symbol_size(symbol_name);
    // x0 = x
    // y0 = y
    // x1 = w
    // y1 = h
    let dst = ViewBox {
        x: 0.0,
        y: CENTER_LINE - (size / 2.0),
        w: size,
        h: size,
    };
    if src.w == 0.0 || src.h == 0.0 {
        return Affine::IDENTITY;
    }
    if dst.w == 0.0 || dst.h == 0.0 {
        return Affine::new([0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    }

    // We follow the same process described in the SVG spec for computing the
    // equivalent scale + translation which maps from viewBox (src) to viewport (dst)
    // coordinates given the value of preserveAspectRatio.
    // https://www.w3.org/TR/SVG/coords.html#ComputingAViewportsTransform
    let sx = dst.w / src.w;
    let sy = dst.h / src.h;

    let tx = dst.x - src.x * sx;
    let ty = dst.y - src.y * sy;

    Affine::new([sx, 0.0, 0.0, sy, tx, ty])
}

#[cfg(test)]
mod tests {
    use super::*;
    use roxmltree::Document;

    fn get_path_d_from_group(svg_content: &str, group_id: &str) -> Option<String> {
        let doc = Document::parse(svg_content).unwrap();
        doc.descendants()
            .find(|n| n.attribute("id") == Some(group_id))
            .and_then(|g| {
                g.descendants()
                    .find(|n| n.has_tag_name("path"))
                    .and_then(|p| p.attribute("d").map(|s| s.to_string()))
            })
    }

    #[test]
    fn test_draw_apple_symbols_sml() {
        let svg_20px = include_str!("../resources/testdata/20px_with_viewbox.svg");
        let svg_24px = include_str!("../resources/testdata/24px.svg");
        let svg_40px = include_str!("../resources/testdata/40px.svg");
        let expected_svg = include_str!("../resources/testdata/regular_sml_baseline.svg");
        let layer_svgs = vec![
            ("Regular-S", svg_20px),
            ("Regular-M", svg_24px),
            ("Regular-L", svg_40px),
        ];

        let actual_svg = draw_apple_symbols(layer_svgs).unwrap();

        let expected_path = get_path_d_from_group(expected_svg, "Regular-L").unwrap();
        let actual_path = get_path_d_from_group(&actual_svg, "Regular-L").unwrap();
        assert_eq!(
            expected_path, actual_path,
            "Path data in Regular-L group does not match"
        );

        assert_eq!(
            actual_svg, expected_svg,
            "Actual SVG does not match expected SVG"
        );
    }

    #[test]
    fn test_draw_apple_symbols_invalid_svg() {
        let svg_24px = include_str!("../resources/testdata/24px_invisible_bounding_box.svg");
        let layer_svgs = vec![("Regular-M", svg_24px)];

        let result = draw_apple_symbols(layer_svgs);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(
            err.to_string(),
            "Invalid SVG: SVG must contain exactly one path element"
        );
    }
}
