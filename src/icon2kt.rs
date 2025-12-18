//! Produces Android Compose ImageVector Kotlin code of icons in Google-style icon fonts

use crate::{draw_glyph::*, error::DrawSvgError};
use skrifa::{raw::TableProvider, FontRef};

pub fn draw_kt(
    font: &FontRef,
    options: &DrawOptions,
    package: &str,
) -> Result<String, DrawSvgError> {
    let upem = font
        .head()
        .map_err(|e| DrawSvgError::ReadError("head", e))?
        .units_per_em();
    let viewbox = options.xml_viewbox(upem);
    let mut pen = get_pen(viewbox, upem);

    draw_glyph(font, options, &mut pen)?;

    let field_name: String = format!("_{}", options.kt_variable_name).to_lowercase();
    let color = options
        .fill_color
        // our input is rgba, kt Color takes argb
        // https://developer.android.com/reference/kotlin/androidx/compose/ui/graphics/Color#representation
        .map(|c| c.rotate_right(8))
        .map(|c| format!("Color({:#010x})", c))
        .unwrap_or("Color.Black".to_string());
    let path_data = options.style.write_kt_path(&pen.into_inner());
    let mut additional_attributes = String::new();
    for attr in &options.additional_attributes {
        additional_attributes.push_str("          ");
        additional_attributes.push_str(attr);
        additional_attributes.push_str(",\n");
    }

    let kt = format!(
        r#"package {package}

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.PathFillType
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.path
import androidx.compose.ui.unit.dp

@Suppress("CheckReturnValue")
public val {icon_name}: ImageVector
  get() {{
    if ({field_name} != null) {{
      return {field_name}!!
    }}
    {field_name} =
      ImageVector.Builder(
          name = "{icon_name}",
          defaultWidth = {width_dp}.dp,
          defaultHeight = {height_dp}.dp,
          viewportWidth = {viewport_width}f,
          viewportHeight = {viewport_height}f,
{additional_attributes}        )
        .apply {{
          path(
            fill = SolidColor({color}),
            fillAlpha = 1f,
            stroke = null,
            strokeAlpha = 1f,
            strokeLineWidth = 1f,
            strokeLineCap = StrokeCap.Butt,
            strokeLineJoin = StrokeJoin.Bevel,
            strokeLineMiter = 1f,
            pathFillType = PathFillType.Companion.NonZero,
          ) {{
{path_data}          }}
        }}
        .build()
    return {field_name}!!
  }}

private var {field_name}: ImageVector? = null
"#,
        icon_name = options.kt_variable_name,
        width_dp = options.width_height,
        height_dp = options.width_height,
        viewport_width = viewbox.width,
        viewport_height = viewbox.height,
    );

    Ok(kt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{iconid, pathstyle::SvgPathStyle, testdata};
    use skrifa::{FontRef, MetadataProvider};
    #[test]
    fn draw_mail_icon_kt() {
        let font = FontRef::new(testdata::ICON_FONT).unwrap();
        let loc = font.axes().location(&[
            ("wght", 400.0),
            ("opsz", 24.0),
            ("GRAD", 0.0),
            ("FILL", 1.0),
        ]);
        let options = DrawOptions {
            use_width_height_for_viewbox: true,
            kt_variable_name: "Mail",
            ..DrawOptions::new(
                iconid::MAIL.clone(),
                24.0,
                (&loc).into(),
                SvgPathStyle::Compact(2),
            )
        };

        let actual_kt = draw_kt(&font, &options, "com.example.test").unwrap();

        assert_eq!(testdata::MAIL_KT.trim(), actual_kt.trim());
    }

    fn test_draw_kt(fill: Option<u32>, auto_mirror: bool, expected: &str) {
        let font = FontRef::new(testdata::ICON_FONT).unwrap();
        let loc = font.axes().location(&[
            ("wght", 400.0),
            ("opsz", 24.0),
            ("GRAD", 0.0),
            ("FILL", 1.0),
        ]);
        let options = DrawOptions {
            use_width_height_for_viewbox: true,
            kt_variable_name: "mail",
            fill_color: fill,
            additional_attributes: if auto_mirror {
                vec!["autoMirror = true".to_string()]
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

        let actual_kt = draw_kt(&font, &options, "com.example.test").unwrap();

        assert!(
            actual_kt.contains(expected),
            "expected '{}' in xml: {}",
            expected,
            actual_kt
        );
    }

    #[test]
    fn draw_mail_icon_with_fill() {
        // RRGGBBAA: red=0x11, green=0x22, blue=0x33, alpha=0xff
        test_draw_kt(None, false, "fill = SolidColor(Color.Black),");
        test_draw_kt(Some(0xfa), false, "fill = SolidColor(Color(0xfa000000)),");
        test_draw_kt(
            Some(0x12345678),
            false,
            "fill = SolidColor(Color(0x78123456)),",
        );
    }

    #[test]
    fn draw_mail_auto_mirror() {
        test_draw_kt(
            None,
            true,
            r#"ImageVector.Builder(
          name = "mail",
          defaultWidth = 24.dp,
          defaultHeight = 24.dp,
          viewportWidth = 24f,
          viewportHeight = 24f,
          autoMirror = true,
        )"#,
        );
    }
}
