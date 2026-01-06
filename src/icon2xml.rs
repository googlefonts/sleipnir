//! Produces Android Vector Drawable XML of icons in Google-style icon fonts

use crate::{draw_glyph::*, error::DrawSvgError, pathstyle::SvgPathStyle, xml_element::XmlElement};
use skrifa::{raw::TableProvider, FontRef};

pub fn draw_xml(font: &FontRef, options: &DrawOptions) -> Result<String, DrawSvgError> {
    let upem = font
        .head()
        .map_err(|e| DrawSvgError::ReadError("head", e))?
        .units_per_em();
    let viewbox = options.xml_viewbox(upem);
    let mut pen = get_pen(viewbox, upem);
    let fill_color = options
        .fill_color
        // our input is rgba, VectorDrawablePath_fillColor takes #argb
        // https://developer.android.com/reference/android/R.styleable#VectorDrawablePath_fillColor
        .map(|c| c.rotate_right(8))
        .map(|c| format!("#{:08x}", c))
        .unwrap_or("@android:color/black".to_string());

    draw_glyph(font, options, &mut pen)?;

    let mut vector = XmlElement::new("vector")
        .with_attribute(
            "xmlns:android",
            "http://schemas.android.com/apk/res/android",
        )
        .with_attribute("android:width", format!("{}dp", options.width_height))
        .with_attribute("android:height", format!("{}dp", options.width_height))
        .with_attribute("android:viewportWidth", viewbox.width)
        .with_attribute("android:viewportHeight", viewbox.height)
        .with_child(
            XmlElement::new("path")
                .with_attribute("android:fillColor", fill_color)
                .with_attribute(
                    "android:pathData",
                    SvgPathStyle::Compact(2).write_svg_path(&pen.into_inner()),
                ),
        );

    for attr in &options.additional_attributes {
        if let Some((name, value)) = attr.split_once('=') {
            vector.add_attribute(name, value.trim_matches('"'));
        }
    }

    Ok(format!("{:#4}", vector))
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
            SvgPathStyle::Compact(2),
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
        let options = DrawOptions {
            use_width_height_for_viewbox: true,
            ..DrawOptions::new(
                iconid::MAIL.clone(),
                24.0,
                (&loc).into(),
                SvgPathStyle::Compact(2),
            )
        };

        let actual_xml = draw_xml(&font, &options).unwrap();
        assert_eq!(testdata::MAIL_VIEWBOX_XML.trim(), actual_xml.trim());
    }

    #[track_caller]
    fn test_draw_xml(fill: Option<u32>, auto_mirror: bool, expected: &str) {
        let font = FontRef::new(testdata::ICON_FONT).unwrap();
        let loc = font.axes().location(&[
            ("wght", 400.0),
            ("opsz", 24.0),
            ("GRAD", 0.0),
            ("FILL", 1.0),
        ]);
        let options = DrawOptions {
            fill_color: fill,
            additional_attributes: if auto_mirror {
                vec!["android:autoMirrored=\"true\"".to_string()]
            } else {
                vec![]
            },
            ..DrawOptions::new(
                iconid::MAIL.clone(),
                24.0,
                (&loc).into(),
                SvgPathStyle::Unchanged(2),
            )
        };

        let actual_svg = draw_xml(&font, &options).unwrap();

        assert!(
            actual_svg.contains(expected),
            "expected '{}' in xml: {}",
            expected,
            actual_svg
        );
    }

    #[test]
    fn draw_mail_icon_with_fill() {
        // RRGGBBAA: red=0x11, green=0x22, blue=0x33, alpha=0xff
        test_draw_xml(None, false, "android:fillColor=\"@android:color/black\"");
        test_draw_xml(Some(0xfa), false, "android:fillColor=\"#fa000000\"");
        test_draw_xml(Some(0x12345678), false, "android:fillColor=\"#78123456\"");
    }

    #[test]
    fn draw_mail_icon_with_auto_mirror() {
        test_draw_xml(
            None,
            true,
            r#"<vector xmlns:android="http://schemas.android.com/apk/res/android"
    android:width="24dp"
    android:height="24dp"
    android:viewportWidth="960"
    android:viewportHeight="960"
    android:autoMirrored="true">
"#,
        );
    }
}
