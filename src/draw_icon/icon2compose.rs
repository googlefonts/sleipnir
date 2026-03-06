//! Produces Android Compose ImageVector Kotlin code of icons in Google-style icon fonts

use super::{draw_glyph, DrawOptions, DrawType, DrawingInstructions, GlyphType};
use crate::draw_icon::get_pen;
use crate::error::DrawSvgError;

pub(super) fn draw_compose_image_vector(
    drawing_instructions: DrawingInstructions,
    options: &DrawOptions,
) -> Result<String, DrawSvgError> {
    let mut pen = get_pen(drawing_instructions.viewbox, drawing_instructions.upem);

    match drawing_instructions.glyph {
        GlyphType::Outline(glyph) => draw_glyph(glyph, options, &mut pen)?,
        GlyphType::Color(_glyph) => {
            return Err(DrawSvgError::ColorGlyphNotSupported(
                drawing_instructions.glyph_id,
            ))
        }
    }

    let DrawType::ComposeImageVector {
        variable_name,
        package,
    } = &options.draw_type
    else {
        return Err(DrawSvgError::UnExpectedDrawType(format!(
            "Expected ComposeImageVector, Got: {:?}",
            options.draw_type
        )));
    };

    let field_name: String = format!("_{}", variable_name.to_lowercase().replace(".", "_"));
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
public val {variable_name}: ImageVector
  get() {{
    if ({field_name} != null) {{
      return {field_name}!!
    }}
    {field_name} =
      ImageVector.Builder(
          name = "{variable_name}",
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
        package = package,
        variable_name = variable_name,
        width_dp = drawing_instructions.glyph_width,
        height_dp = options.height,
        viewport_width = drawing_instructions.viewbox.width,
        viewport_height = drawing_instructions.viewbox.height,
    );

    Ok(kt)
}

#[cfg(test)]
mod tests {
    use crate::draw_icon::{DrawIcon, DrawOptions, DrawType, ViewBoxMode};
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
            viewbox_mode: ViewBoxMode::UseHeight,

            ..DrawOptions::new(
                iconid::MAIL.clone(),
                24.0,
                (&loc).into(),
                SvgPathStyle::Compact(2),
                DrawType::ComposeImageVector {
                    variable_name: "Mail",
                    package: "com.example.test",
                },
            )
        };

        let actual_kt = font.draw_icon(&options).unwrap();

        assert_eq!(testdata::MAIL_KT.trim(), actual_kt.trim());
    }

    #[test]
    fn test_draw_kt_with_fill() {
        // RRGGBBAA: red=0x11, green=0x22, blue=0x33, alpha=0xff
        test_draw_kt("mail", None, false, "fill = SolidColor(Color.Black),");
        test_draw_kt(
            "mail",
            Some(0xfa),
            false,
            "fill = SolidColor(Color(0xfa000000)),",
        );
        test_draw_kt(
            "mail",
            Some(0x12345678),
            false,
            "fill = SolidColor(Color(0x78123456)),",
        );
    }

    #[test]
    fn test_draw_kt_auto_mirror() {
        test_draw_kt(
            "mail",
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

    #[test]
    fn test_draw_kt_variable_name() {
        test_draw_kt(
            "mail.default",
            None,
            true,
            "public val mail.default: ImageVector
  get() {
    if (_mail_default != null) {
      return _mail_default!!
    }",
        );
    }

    fn test_draw_kt(name: &str, fill: Option<u32>, auto_mirror: bool, expected: &str) {
        let font = FontRef::new(testdata::ICON_FONT).unwrap();
        let loc = font.axes().location(&[
            ("wght", 400.0),
            ("opsz", 24.0),
            ("GRAD", 0.0),
            ("FILL", 1.0),
        ]);
        let options = DrawOptions {
            viewbox_mode: ViewBoxMode::UseHeight,
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
                DrawType::ComposeImageVector {
                    variable_name: name,
                    package: "com.example.test",
                },
            )
        };

        let actual_kt = font.draw_icon(&options).unwrap();

        assert!(
            actual_kt.contains(expected),
            "expected '{}' in xml: {}",
            expected,
            actual_kt
        );
    }
}
