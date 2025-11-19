//! Produces Android Compose ImageVector Kotlin code of icons in Google-style icon fonts

use crate::{draw_glyph::*, error::DrawSvgError};
use skrifa::{raw::TableProvider, FontRef};

fn snake_to_upper_camel(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = true;
    for c in s.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            if c.is_ascii_digit() {
                result.push('_');
                result.push(c);
            } else {
                result.extend(c.to_uppercase());
            }
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }
    result
}

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

    let icon_camel = snake_to_upper_camel(options.icon_name);
    let field_name = format!("_{}", options.icon_name.replace('_', ""));

    let color = options
        .fill_color
        // our input is rgba, kt Color takes argb
        // https://developer.android.com/reference/kotlin/androidx/compose/ui/graphics/Color#representation
        .map(|c| c.rotate_right(8))
        .map(|c| format!("Color({:#010x})", c))
        .unwrap_or("Color.Black".to_string());
    let path_data = options.style.write_kt_path(&pen.into_inner());

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
public val {icon_camel}: ImageVector
  get() {{
    if ({field_name} != null) {{
      return {field_name}!!
    }}
    {field_name} =
      ImageVector.Builder(
          name = "{icon_camel}",
          defaultWidth = {width_dp}.dp,
          defaultHeight = {height_dp}.dp,
          viewportWidth = {viewport_width}f,
          viewportHeight = {viewport_height}f,
        )
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
        let mut options = DrawOptions::new(
            iconid::MAIL.clone(),
            24.0,
            (&loc).into(),
            SvgPathStyle::Compact(2),
        );
        options.use_width_height_for_viewbox = true;
        options.icon_name = "mail";

        let actual_kt = draw_kt(&font, &options, "com.example.test").unwrap();
        assert_eq!(testdata::MAIL_KT.trim(), actual_kt.trim());
    }

    #[test]
    fn test_snake_to_upper_camel() {
        assert_eq!(snake_to_upper_camel("foo"), "Foo");
        assert_eq!(snake_to_upper_camel("foo_bar"), "FooBar");
        assert_eq!(snake_to_upper_camel("3d_rotation"), "_3dRotation");
        assert_eq!(snake_to_upper_camel("123_foo"), "_123Foo");
        assert_eq!(snake_to_upper_camel("foo_123"), "Foo_123");
        assert_eq!(snake_to_upper_camel("_foo"), "Foo");
        assert_eq!(snake_to_upper_camel("__foo"), "Foo");
        assert_eq!(snake_to_upper_camel("foo__bar"), "FooBar");
    }

    fn test_color(fill: Option<u32>, expected: &str) {
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
            SvgPathStyle::Unchanged(2),
        );
        options.use_width_height_for_viewbox = true;
        options.icon_name = "mail";
        options.fill_color = fill;

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
        test_color(None, "fill = SolidColor(Color.Black),");
        test_color(Some(0xfa), "fill = SolidColor(Color(0xfa000000)),");
        test_color(Some(0x12345678), "fill = SolidColor(Color(0x78123456)),");
    }
}
