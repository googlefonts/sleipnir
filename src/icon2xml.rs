//! Produces Android Vector Drawable XML of icons in Google-style icon fonts

use crate::{
    error::DrawSvgError, icon2svg::DrawOptions, pathstyle::SvgPathStyle, pens::SvgPathPen,
};
use kurbo::Affine;
use skrifa::{
    instance::Size as SkrifaSize, outline::pen::PathStyle, outline::DrawSettings,
    raw::TableProvider, FontRef, MetadataProvider,
};

pub fn draw_xml(font: &FontRef, options: &DrawOptions<'_>) -> Result<String, DrawSvgError> {
    let upem = font
        .head()
        .map_err(|e| DrawSvgError::ReadError("head", e))?
        .units_per_em();
    let gid = options
        .identifier
        .resolve(font, &options.location)
        .map_err(|e| DrawSvgError::ResolutionError(options.identifier.clone(), e))?;

    let glyph = font
        .outline_glyphs()
        .get(gid)
        .ok_or(DrawSvgError::NoOutline(options.identifier.clone(), gid))?;

    let (viewport_width, viewport_height) = if options.use_width_height_for_viewbox {
        (options.width_height, options.width_height)
    } else {
        (upem as f32, upem as f32)
    };
    let scale = viewport_width as f64 / upem as f64;

    // Fonts are Y-up, vector drawable Y-down.
    // So we need to flip y and translate.
    // y' = viewport_height - y_glyph * scale
    // x' = x_glyph * scale
    let transform = Affine::new([scale, 0.0, 0.0, -scale, 0.0, viewport_height as f64]);
    let mut pen = SvgPathPen::new_with_transform(transform);

    glyph
        .draw(
            DrawSettings::unhinted(SkrifaSize::unscaled(), options.location)
                .with_path_style(PathStyle::HarfBuzz),
            &mut pen,
        )
        .map_err(|e| DrawSvgError::DrawError(options.identifier.clone(), gid, e))?;

    let mut xml = format!(
        "<vector xmlns:android=\"http://schemas.android.com/apk/res/android\"\n    android:width=\"{width_dp}dp\"\n    android:height=\"{height_dp}dp\"\n    android:viewportWidth=\"{viewport_width}\"\n    android:viewportHeight=\"{viewport_height}\"",
        width_dp = options.width_height as u32,
        height_dp = options.width_height as u32,
        viewport_width = viewport_width,
        viewport_height = viewport_height
    );

    for attr in &options.additional_attributes {
        xml.push_str("\n    ");
        xml.push_str(attr);
    }
    xml.push_str(">\n");

    xml.push_str(&format!(
        "    <path\n        android:fillColor=\"@android:color/white\"\n        android:pathData=\"{}\"/>\n",
        SvgPathStyle::Compact.write_svg_path(&pen.into_inner())
    ));

    xml.push_str("</vector>");

    Ok(xml)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icon2svg::DrawOptions;
    use crate::iconid;
    use crate::testdata;
    use skrifa::FontRef;

    #[test]
    fn draw_mail_icon_xml() {
        let font = FontRef::new(testdata::ICON_FONT).unwrap();
        let loc = font.axes().location(&[
            ("wght", 400.0),
            ("opsz", 24.0),
            ("GRAD", 0.0),
            ("FILL", 1.0),
        ]);
        let options = DrawOptions::new(
            iconid::MAIL.clone(),
            24.0,
            (&loc).into(),
            SvgPathStyle::Compact,
        );

        let actual_xml = draw_xml(&font, &options).unwrap();
        assert_eq!(testdata::MAIL_XML.trim(), actual_xml);
    }

    #[test]
    fn draw_mail_icon_xml_viewbox() {
        let font = FontRef::new(testdata::ICON_FONT).unwrap();
        let loc = font.axes().location(&[
            ("wght", 400.0),
            ("opsz", 24.0),
            ("GRAD", 0.0),
            ("FILL", 1.0),
        ]);
        let mut options = DrawOptions::new(
            iconid::MAIL.clone(),
            24.0,
            (&loc).into(),
            SvgPathStyle::Compact,
        );
        options.use_width_height_for_viewbox = true;

        let actual_xml = draw_xml(&font, &options).unwrap();
        assert_eq!(testdata::MAIL_VIEWBOX_XML.trim(), actual_xml.trim());
    }
}
