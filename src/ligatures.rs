//! Helpers for working with layout

use skrifa::{
    raw::{
        tables::gsub::{Ligature, LigatureSubstFormat1, SubstitutionSubtables},
        FontRef, TableProvider,
    },
    GlyphId, MetadataProvider,
};

use crate::error::IconResolutionError;

pub trait Ligatures {
    /// Exposes the complete set of ligature substitution tables in the font
    fn ligature_substitutions(&self) -> impl Iterator<Item = LigatureSubstFormat1<'_>>;

    /// Returns the first glyph and the [Ligature] containing glyphs 2..n and the substitution target
    fn ligatures(&self) -> impl Iterator<Item = (GlyphId, Ligature<'_>)>;

    /// Resolve a string to the glyph id that will be produced by ligature for that string
    ///
    /// Meant for use with icon names in contexts where speed is not essential.
    fn resolve_ligature(&self, name: &str) -> Result<Option<GlyphId>, IconResolutionError>;
}

impl<'a> Ligatures for FontRef<'a> {
    fn ligature_substitutions(&self) -> impl Iterator<Item = LigatureSubstFormat1<'_>> {
        self.gsub()
            .into_iter()
            .flat_map(|gsub| gsub.lookup_list().into_iter())
            .flat_map(|lookup_list| lookup_list.lookups().iter().flat_map(|l| l.into_iter()))
            .flat_map(|lookup| lookup.subtables().into_iter())
            .filter_map(|subtable| {
                if let SubstitutionSubtables::Ligature(table) = subtable {
                    Some(table)
                } else {
                    None
                }
            })
            .flat_map(|table| table.iter().filter_map(|e| e.ok()))
    }

    fn resolve_ligature(&self, name: &str) -> Result<Option<GlyphId>, IconResolutionError> {
        let charmap = self.charmap();
        let gids = name
            .chars()
            .map(|c| {
                charmap
                    .map(c)
                    .ok_or(IconResolutionError::UnmappedCharError(c))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let Some(first) = gids.first() else {
            return Err(IconResolutionError::NoGlyphIds(name.to_string()));
        };
        let gids = &gids[1..];

        for (liga_first, liga) in self.ligatures() {
            if liga_first != *first {
                continue;
            }
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

    fn ligatures(&self) -> impl Iterator<Item = (GlyphId, Ligature<'_>)> {
        self.ligature_substitutions()
            .filter_map(|liga_subst| liga_subst.coverage().ok().map(|c| (c, liga_subst)))
            .flat_map(|(coverage, liga_subst)| {
                coverage
                    .iter()
                    .filter_map(move |first| coverage.get(first).map(|i| (first, i)))
                    .filter_map(move |(first, set_index)| {
                        liga_subst
                            .ligature_sets()
                            .get(set_index as usize)
                            .map(|set| (first, set))
                            .ok()
                    })
            })
            .flat_map(|(first, set)| {
                set.ligatures()
                    .iter()
                    .filter_map(|liga| liga.ok())
                    .map(move |liga| (first, liga))
            })
    }
}
