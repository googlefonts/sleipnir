//! Identification of icons and resolution of glyph ids. Assumes Google style icon font input.
use skrifa::{
    charmap::Charmap,
    raw::{
        tables::gsub::{ExtensionSubtable, LigatureSubstFormat1, SubstitutionLookup},
        FontRef, TableProvider,
    },
    GlyphId,
};
use smol_str::SmolStr;

use crate::error::IconResolutionError;

#[derive(Clone, Debug)]
pub enum IconIdentifier {
    GlyphId(GlyphId),
    Codepoint(u32),
    Name(SmolStr),
}

impl IconIdentifier {
    pub fn resolve(&self, font: &FontRef) -> Result<GlyphId, IconResolutionError> {
        match self {
            IconIdentifier::GlyphId(gid) => Ok(*gid),
            IconIdentifier::Codepoint(cp) => font
                .cmap()
                .map_err(IconResolutionError::ReadError)?
                .map_codepoint(*cp)
                .ok_or_else(|| IconResolutionError::NoCmapEntry(*cp)),
            IconIdentifier::Name(name) => resolve_icon_ligature(font, name.as_str()),
        }
    }
}

fn resolve_ligature(
    liga: &LigatureSubstFormat1<'_>,
    text: &str,
    gids: &[GlyphId],
) -> Result<Option<GlyphId>, IconResolutionError> {
    let Some(first) = gids.first() else {
        return Err(IconResolutionError::NoGlyphIds(text.to_string()));
    };
    let coverage = liga.coverage().map_err(IconResolutionError::ReadError)?;
    let Some(set_index) = coverage.get(*first) else {
        return Ok(None);
    };
    let set = liga
        .ligature_sets()
        .get(set_index as usize)
        .map_err(IconResolutionError::ReadError)?;
    // Seek a ligature that matches glyphs 2..N of name
    // We don't care about speed
    let gids = &gids[1..];
    for liga in set.ligatures().iter() {
        let liga = liga.map_err(IconResolutionError::ReadError)?;
        if liga.component_count() as usize != gids.len() + 1 {
            continue;
        }
        if gids
            .iter()
            .zip(liga.component_glyph_ids())
            .all(|(gid, component)| *gid == component.get())
        {
            return Ok(Some(liga.ligature_glyph())); // We found it!
        }
    }
    Ok(None)
}

pub fn resolve_icon_ligature(font: &FontRef, name: &str) -> Result<GlyphId, IconResolutionError> {
    let charmap = Charmap::new(font);
    let gids = name
        .chars()
        .map(|c| {
            charmap
                .map(c)
                .ok_or(IconResolutionError::UnmappedCharError(c))
        })
        .collect::<Result<Vec<_>, _>>()?;

    // Step 1: try to find a ligature that starts with our first gid
    let gsub = font.gsub().map_err(IconResolutionError::ReadError)?;
    let lookups = gsub.lookup_list().map_err(IconResolutionError::ReadError)?;
    for lookup in lookups.lookups().iter() {
        let lookup = lookup.map_err(IconResolutionError::ReadError)?;
        match lookup {
            SubstitutionLookup::Ligature(table) => {
                for liga in table.subtables().iter() {
                    let liga = liga.map_err(IconResolutionError::ReadError)?;
                    if let Some(gid) = resolve_ligature(&liga, name, &gids)? {
                        return Ok(gid);
                    }
                }
            }
            SubstitutionLookup::Extension(table) => {
                for lookup in table.subtables().iter() {
                    let ExtensionSubtable::Ligature(table) =
                        lookup.map_err(IconResolutionError::ReadError)?
                    else {
                        continue;
                    };
                    let table = table.extension().map_err(IconResolutionError::ReadError)?;

                    if let Some(gid) = resolve_ligature(&table, name, &gids)? {
                        return Ok(gid);
                    }
                }
            }
            _ => (),
        }
    }
    Err(IconResolutionError::NoLigature(name.to_string()))
}

#[cfg(test)]
pub static MAIL: IconIdentifier = IconIdentifier::Codepoint(57688);
#[cfg(test)]
pub static LAN: IconIdentifier = IconIdentifier::Name(SmolStr::new_static("lan"));
#[cfg(test)]
pub static MAN: IconIdentifier = IconIdentifier::GlyphId(GlyphId::new(5));

#[cfg(test)]
mod tests {
    use skrifa::{FontRef, GlyphId};

    use crate::{
        iconid::{LAN, MAIL, MAN},
        testdata_bytes,
    };

    #[test]
    fn resolve_mail_icon() {
        let raw_font = testdata_bytes("vf[FILL,GRAD,opsz,wght].ttf");
        assert_eq!(
            GlyphId::new(1),
            MAIL.resolve(&FontRef::new(&raw_font).unwrap()).unwrap()
        );
    }

    #[test]
    fn resolve_lan_icon() {
        let raw_font = testdata_bytes("vf[FILL,GRAD,opsz,wght].ttf");
        assert_eq!(
            GlyphId::new(3),
            LAN.resolve(&FontRef::new(&raw_font).unwrap()).unwrap()
        );
    }

    #[test]
    fn resolve_man_icon() {
        let raw_font = testdata_bytes("vf[FILL,GRAD,opsz,wght].ttf");
        assert_eq!(
            GlyphId::new(5),
            MAN.resolve(&FontRef::new(&raw_font).unwrap()).unwrap()
        );
    }
}
