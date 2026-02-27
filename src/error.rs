use crate::{iconid::IconIdentifier, pens::GlyphPainterError};
use skrifa::{
    color::{CompositeMode, PaintError},
    outline::DrawError,
    raw::ReadError,
    GlyphId,
};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DrawSvgError {
    #[error("Invalid SVG: {0}")]
    InvalidSvg(String),
    #[error("Parse Error: {0}")]
    ParseError(String),
    #[error("Unable to determine glyph id for {0:?}: {1}")]
    ResolutionError(IconIdentifier, IconResolutionError),
    #[error("{0:?} ({1}) has no outline")]
    NoOutline(IconIdentifier, GlyphId),
    #[error("{0:?} ({1}) failed to draw: {2}")]
    DrawError(IconIdentifier, GlyphId, DrawError),
    #[error("{0:?} ({1}) failed to paint: {2}")]
    PaintError(IconIdentifier, GlyphId, PaintError),
    #[error("{0}")]
    PainterError(#[from] GlyphPainterError),
    #[error("Unable to read {0}: {1}")]
    ReadError(&'static str, skrifa::raw::ReadError),
    #[error("Unsupported SVG feature: sweep gradient")]
    SweepGradientNotSupported,
    #[error("Unsupported SVG feature: composite mode {0:?}")]
    CompositeModeNotSupported(CompositeMode),
}

#[derive(Debug, Error)]
pub enum IconResolutionError {
    #[error("{0}")]
    ReadError(ReadError),
    #[error("No character mapping for '{0}'")]
    UnmappedCharError(char),
    #[error("The icon name '{0}' resolved to 0 glyph ids")]
    NoGlyphIds(String),
    #[error("The icon name '{0}' has no ligature")]
    NoLigature(String),
    #[error("The codepoint 0x{0:04x} has no cmap entry")]
    NoCmapEntry(u32),
    #[error("The gid '{0}' has no cmap entry.")]
    NoCmapEntryForGid(u32),
    #[error("codepoint '{0}' doesn't map to a valid character")]
    InvalidCharacter(u32),
    #[error("'{0}'")]
    Invalid(String),
    #[error("Big glyph ids not yet supported, unable to handle {0}")]
    BigGid(GlyphId),
}

impl From<ReadError> for IconResolutionError {
    fn from(obj: ReadError) -> Self {
        Self::ReadError(obj)
    }
}
