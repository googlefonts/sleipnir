//! Produces Android Vector Drawable XML of icons in Google-style icon fonts

use crate::{draw_glyph::*, error::DrawSvgError, pathstyle::SvgPathStyle};
use skrifa::{raw::TableProvider, FontRef};

pub fn draw_xml(font: &FontRef, options: &DrawOptions) -> Result<String, DrawSvgError> {
    let upem = font
        .head()
        .map_err(|e| DrawSvgError::ReadError("head", e))?
        .units_per_em();
    let viewbox = options.xml_viewbox(upem);
    let mut pen = get_pen(viewbox, upem);

    draw_glyph(font, options, &mut pen)?;

    let mut xml = format!(
        "<vector xmlns:android=\"http://schemas.android.com/apk/res/android\"\n    android:width=\"{width_dp}dp\"\n    android:height=\"{height_dp}dp\"\n    android:viewportWidth=\"{viewport_width}\"\n    android:viewportHeight=\"{viewport_height}\"",
        width_dp = options.width_height as u32,
        height_dp = options.width_height as u32,
        viewport_width = viewbox.width,
        viewport_height = viewbox.height
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
    use crate::{iconid, testdata};
    use skrifa::{FontRef, MetadataProvider};
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
