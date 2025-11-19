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
    // What icon are we drawing from the font.
    pub identifier: IconIdentifier,
    // The width and height of the icon in px for Svgs, and dp for android vd and Kt.
    pub width_height: f32,
    // The axis location to use when drawing the path.
    pub location: LocationRef<'a>,
    pub style: SvgPathStyle,
    // If true, the viewbox will be set to x=0,y=0, width=width_height, height=width_height.
    // If false, the viewbox will be set to x=0,y=-upem, width=upem, height=upem.
    pub use_width_height_for_viewbox: bool,
    pub additional_attributes: Vec<String>,
    // The icon name to use in the generated Kotlin code, in snake_case format.
    pub icon_name: &'a str,
    // Color to fill the icon, 32-bit encoded as RRGGBBAA.
    pub fill_color: Option<u32>,
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
            icon_name: "",
            fill_color: None,
        }
    }

    pub(crate) fn svg_viewbox(&self, upem: u16) -> ViewBox {
        if self.use_width_height_for_viewbox {
            ViewBox {
                x: 0.0,
                y: 0.0,
                width: self.width_height as f64,
                height: self.width_height as f64,
            }
        } else {
            ViewBox {
                x: 0.0,
                y: -(upem as f64),
                width: upem as f64,
                height: upem as f64,
            }
        }
    }
    pub(crate) fn xml_viewbox(&self, upem: u16) -> ViewBox {
        // VectorDrawable's viewport always starts at (0, 0)
        if self.use_width_height_for_viewbox {
            ViewBox {
                x: 0.0,
                y: 0.0,
                width: self.width_height as f64,
                height: self.width_height as f64,
            }
        } else {
            ViewBox {
                x: 0.0,
                y: 0.0,
                width: upem as f64,
                height: upem as f64,
            }
        }
    }
}

pub(crate) fn get_pen(viewbox: ViewBox, upem: u16) -> SvgPathPen {
    let scale = viewbox.width / upem as f64;
    // Font Coordinates: use a Y-up system. The origin (0,0) is at the bottom-left corner,
    // and Y values increase upwards.
    // Svg Coordinates: Use a Y-down system. The origin (0,0) is at the top-left corner,
    // and Y values increase downwards.
    let translate_y = viewbox.height + viewbox.y;
    SvgPathPen::new_with_transform(Affine::new([scale, 0.0, 0.0, -scale, 0.0, translate_y]))
}

#[derive(Copy, Clone)]
pub(crate) struct ViewBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
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
