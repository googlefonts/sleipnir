use crate::{
    error::DrawSvgError, iconid::IconIdentifier, pathstyle::SvgPathStyle, pens::SvgPathPen,
};
use kurbo::Affine;
use skrifa::{
    instance::{LocationRef, Size},
    outline::{pen::PathStyle, DrawSettings, OutlinePen},
    FontRef, MetadataProvider,
};

pub struct DrawOptions<'a> {
    pub identifier: IconIdentifier,
    pub width_height: f32,
    pub location: LocationRef<'a>,
    pub style: SvgPathStyle,
    pub use_width_height_for_viewbox: bool,
    pub additional_attributes: Vec<&'a str>,
}

impl<'a> DrawOptions<'a> {
    pub fn new(
        identifier: IconIdentifier,
        width_height: f32,
        location: LocationRef<'a>,
        style: SvgPathStyle,
    ) -> DrawOptions<'a> {
        DrawOptions {
            identifier,
            width_height,
            location,
            style,
            use_width_height_for_viewbox: false,
            additional_attributes: Vec::new(),
        }
    }

    pub(crate) fn svg_viewbox(&self, upem: u16) -> ViewBox {
        if self.use_width_height_for_viewbox {
            ViewBox {
                x: 0.0,
                y: 0.0,
                width: self.width_height,
                height: self.width_height,
            }
        } else {
            ViewBox {
                x: 0.0,
                y: -(upem as f32),
                width: upem as f32,
                height: upem as f32,
            }
        }
    }
    pub(crate) fn xml_viewbox(&self, upem: u16) -> ViewBox {
        if self.use_width_height_for_viewbox {
            ViewBox {
                x: 0.0,
                y: 0.0,
                width: self.width_height,
                height: self.width_height,
            }
        } else {
            ViewBox {
                x: 0.0,
                y: 0.0,
                width: upem as f32,
                height: upem as f32,
            }
        }
    }
}

pub(crate) fn get_pen(viewbox: ViewBox, upem: u16) -> SvgPathPen {
    let scale = viewbox.width as f64 / upem as f64;
    // Font Coordinates: use a Y-up system. The origin (0,0) is at the bottom-left corner,
    // and Y values increase upwards.
    // SVG Coordinates: Use a Y-down system. The origin (0,0) is at the top-left corner,
    // and Y values increase downwards.
    let translate_y = viewbox.height + viewbox.y;
    SvgPathPen::new_with_transform(Affine::new([
        scale,
        0.0,
        0.0,
        -scale,
        0.0,
        translate_y.into(),
    ]))
}

#[derive(Copy, Clone)]
pub(crate) struct ViewBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

pub(crate) fn draw_glyph(
    font: &FontRef,
    options: &DrawOptions<'_>,
    pen: &mut impl OutlinePen,
) -> Result<(), DrawSvgError> {
    let gid = options
        .identifier
        .resolve(font, &options.location)
        .map_err(|e| DrawSvgError::ResolutionError(options.identifier.clone(), e))?;

    let glyph = font
        .outline_glyphs()
        .get(gid)
        .ok_or(DrawSvgError::NoOutline(options.identifier.clone(), gid))?;

    glyph
        .draw(
            DrawSettings::unhinted(Size::unscaled(), options.location)
                .with_path_style(PathStyle::HarfBuzz),
            pen,
        )
        .map_err(|e| DrawSvgError::DrawError(options.identifier.clone(), gid, e))?;
    Ok(())
}
