mod icon2compose;
mod icon2svg;
mod icon2xml;

use crate::{
    error::DrawSvgError, iconid::IconIdentifier, pathstyle::SvgPathStyle, pens::SvgPathPen,
};
use kurbo::Affine;
use skrifa::raw::TableProvider;
use skrifa::{
    color::ColorGlyph,
    instance::{LocationRef, Size},
    metrics::{BoundingBox, GlyphMetrics},
    outline::{pen::PathStyle, DrawSettings, OutlinePen},
    FontRef, GlyphId, MetadataProvider, OutlineGlyph,
};
pub struct DrawOptions<'a> {
    // What icon are we drawing from the font.
    pub identifier: IconIdentifier,
    // The height of the icon in px for Svgs, and dp for android vd and Kt.
    pub height: f32,
    // The axis location to use when drawing the path.
    pub location: LocationRef<'a>,
    pub style: SvgPathStyle,
    // If true, the viewbox will be set to x=0,y=0, width=advanced_width, height=width_height.
    pub viewbox_mode: ViewBoxMode,
    pub additional_attributes: Vec<String>,
    // The variable name to use in the generated Kotlin code.
    pub kt_variable_name: &'a str,
    // The package name to use in the generated Kotlin code.
    pub kt_package: &'a str,
    // Color to fill the icon, 32-bit encoded as RRGGBBAA.
    pub fill_color: Option<u32>,

    pub draw_type: DrawType,
}

pub enum DrawType {
    Svg,
    AndroidVectorDrawable,
    ComposeImageVector,
}

pub enum ViewBoxMode {
    // Use the font's htea ascender and descender to determine the viewbox height
    Auto,
    // Use the provided height for the viewbox.
    UseHeight,
    // Use the glyph bounding box for the viewbox.
    // This is the most expensive option, but produces the tightest viewbox.
    UseBoundingBox,
}

impl<'a> DrawOptions<'a> {
    pub fn new(
        identifier: IconIdentifier,
        height: f32,
        location: LocationRef<'a>,
        style: SvgPathStyle,
        draw_type: DrawType,
    ) -> DrawOptions<'a> {
        DrawOptions {
            identifier,
            height,
            location,
            style,
            viewbox_mode: ViewBoxMode::Auto,
            additional_attributes: Vec::new(),
            kt_variable_name: "",
            kt_package: "",
            fill_color: None,
            draw_type,
        }
    }
    fn viewbox_for_svg(
        &self,
        bounding_box: Option<BoundingBox>,
        upem: u16,
        advance_height: f64,
        advance_width: f64,
    ) -> ViewBox {
        match self.viewbox_mode {
            ViewBoxMode::Auto => self.viewbox_auto(advance_height, advance_width),
            ViewBoxMode::UseHeight => ViewBox {
                x: 0.0,
                y: 0.0,
                width: (advance_width * (self.height / upem as f32) as f64).round(),
                height: self.height as f64,
            },
            ViewBoxMode::UseBoundingBox => {
                if let Some(bbox) = bounding_box {
                    let width = bbox.x_max - bbox.x_min;
                    let height = bbox.y_max - bbox.y_min;
                    ViewBox {
                        x: bbox.x_min as f64,
                        y: -bbox.y_max as f64,
                        width: width as f64,
                        height: height as f64,
                    }
                } else {
                    // Fallback to auto viewbox if bounding box is not available.
                    self.viewbox_auto(advance_height, advance_width)
                }
            }
        }
    }

    fn viewbox_auto(&self, advance_height: f64, advance_width: f64) -> ViewBox {
        ViewBox {
            x: 0.0,
            y: -advance_height,
            width: advance_width,
            height: advance_height,
        }
    }
    fn viewbox_for_android(
        &self,
        bounding_box: Option<BoundingBox>,
        upem: u16,
        advance_height: f64,
        advance_width: f64,
    ) -> ViewBox {
        ViewBox {
            x: 0.0,
            y: 0.0,
            ..self.viewbox_for_svg(bounding_box, upem, advance_height, advance_width)
        }
    }

    fn prepare_drawing_instructions(
        &self,
        font: &FontRef<'a>,
    ) -> Result<DrawingInstructions<'a>, DrawSvgError> {
        let gid = self
            .identifier
            .resolve(font, self.location)
            .map_err(|e| DrawSvgError::ResolutionError(self.identifier.clone(), e))?;
        let head = font
            .head()
            .map_err(|e| DrawSvgError::ReadError("head", e))?;
        let upem = head.units_per_em();
        let metrics = GlyphMetrics::new(font, Size::unscaled(), self.location);
        let advance_width = metrics.advance_width(gid).unwrap_or(upem as f32);
        let hhea = font
            .hhea()
            .map_err(|e| DrawSvgError::ReadError("hhea", e))?;
        let advance_height = (hhea.ascender().to_i16() + hhea.descender().to_i16()) as f64;

        let glyph = font
            .color_glyphs()
            .get(gid)
            .map(GlyphType::Color)
            .or_else(|| font.outline_glyphs().get(gid).map(GlyphType::Outline))
            .ok_or(DrawSvgError::NoOutline(self.identifier.clone(), gid))?;
        let bounding_box = match glyph {
            GlyphType::Outline(ref _glyph) => metrics.bounds(gid),
            GlyphType::Color(ref color_glyph) => {
                color_glyph.bounding_box(self.location, Size::unscaled())
            }
        };
        let viewbox = match self.draw_type {
            DrawType::Svg => {
                self.viewbox_for_svg(bounding_box, upem, advance_height, advance_width.into())
            }
            DrawType::AndroidVectorDrawable => {
                self.viewbox_for_android(bounding_box, upem, advance_height, advance_width.into())
            }
            DrawType::ComposeImageVector => {
                self.viewbox_for_android(bounding_box, upem, advance_height, advance_width.into())
            }
        };
        let glyph_width = (self.height as f64 * (viewbox.width / viewbox.height)) as u16;

        Ok(DrawingInstructions {
            glyph,
            viewbox,
            upem,
            glyph_id: gid,
            glyph_width,
        })
    }
}

#[allow(clippy::large_enum_variant)]
pub(crate) enum GlyphType<'a> {
    Outline(OutlineGlyph<'a>),
    Color(ColorGlyph<'a>),
}
pub(crate) struct DrawingInstructions<'a> {
    // The glyph to draw. This is either an outline glyph or a color glyph.
    pub glyph: GlyphType<'a>,
    // The glyph ID of the glyph to draw. This is used for error reporting and for some font operations.
    pub glyph_id: GlyphId,
    // The viewbox to use for the output. This is calculated based on the options and the font metrics.
    pub viewbox: ViewBox,
    // Units per em of the font, used to calculate the scale factor for the output.
    pub upem: u16,
    // The width of the glyph in user space units. This is used to calculate the viewbox and the scale factor for the output.
    pub glyph_width: u16,
}
pub trait DrawIcon {
    fn draw_icon(&self, options: &DrawOptions) -> Result<String, DrawSvgError>;
}
impl DrawIcon for FontRef<'_> {
    fn draw_icon(&self, options: &DrawOptions) -> Result<String, DrawSvgError> {
        let di = options.prepare_drawing_instructions(self)?;
        match options.draw_type {
            DrawType::Svg => icon2svg::draw_svg(self, di, options),
            DrawType::AndroidVectorDrawable => icon2xml::draw_android_vector_drawable(di, options),
            DrawType::ComposeImageVector => icon2compose::draw_compose_image_vector(di, options),
        }
    }
}

fn get_pen(viewbox: ViewBox, upem: u16) -> SvgPathPen {
    let scale = viewbox.height / upem as f64;
    // Font Coordinates: use a Y-up system. The origin (0,0) is at the bottom-left corner,
    // and Y values increase upwards.
    // Svg Coordinates: Use a Y-down system. The origin (0,0) is at the top-left corner,
    // and Y values increase downwards.
    let translate_y = viewbox.height + viewbox.y;

    SvgPathPen::new_with_transform(Affine::new([scale, 0.0, 0.0, -scale, 0.0, translate_y]))
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct ViewBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

fn draw_glyph(
    glyph: OutlineGlyph<'_>,
    options: &DrawOptions<'_>,
    pen: &mut impl OutlinePen,
) -> Result<(), DrawSvgError> {
    glyph
        .draw(
            DrawSettings::unhinted(Size::unscaled(), options.location)
                .with_path_style(PathStyle::HarfBuzz),
            pen,
        )
        .map_err(|e| DrawSvgError::DrawError(options.identifier.clone(), glyph.glyph_id(), e))?;
    Ok(())
}
