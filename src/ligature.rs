use skrifa::{
    charmap::Charmap,
    raw::{
        tables::gsub::{CoverageTable, LigatureMarker, LigatureSet, SubstitutionSubtables},
        types::Offset16,
        ArrayOfOffsets, FontRef, ReadError, TableProvider, TableRef,
    },
    GlyphId,
};
use std::{cell::RefCell, iter};

use crate::error::IconResolutionError;

pub struct Ligature {
    // Includes all components, not just 1..N.
    pub components: Vec<GlyphId>,
    pub glyph: GlyphId,
}

type LigatureInnerIter<'a> = Vec<(
    GlyphId, // First gid in ligature components.
    RefCell<Box<dyn Iterator<Item = Result<TableRef<'a, LigatureMarker>, ReadError>> + 'a>>,
)>;
pub struct LigatureIter<'a> {
    inner_iter: LigatureInnerIter<'a>,
    // current position in `inner_iter`
    idx: usize,
    // 1..N, if present, the iterator will be targeted to return only ligatures matching `gids`
    gids: Option<Vec<GlyphId>>,
}

impl<'a> Iterator for LigatureIter<'a> {
    type Item = Result<Ligature, IconResolutionError>;
    fn next(&mut self) -> Option<Result<Ligature, IconResolutionError>> {
        while self.idx < self.inner_iter.len() {
            let (gid, ligatures_iter) = self.inner_iter.get(self.idx)?;
            let ligature = match ligatures_iter.borrow_mut().next() {
                Some(Ok(e)) => e,
                Some(Err(e)) => {
                    self.idx = self.inner_iter.len();
                    return Some(Err(e.into()));
                }
                None => {
                    self.idx += 1;
                    continue;
                }
            };
            if let Some(ref gids) = self.gids {
                if ligature.component_count() as usize != gids.len() + 1
                    || !gids
                        .iter()
                        .zip(ligature.component_glyph_ids())
                        .all(|(gid, component)| *gid == component.get())
                {
                    continue;
                }
            }

            let components = iter::once(*gid)
                .chain(ligature.component_glyph_ids().iter().map(|f| f.get()))
                .collect::<Vec<_>>();

            return Some(Ok(Ligature {
                components,
                glyph: ligature.ligature_glyph(),
            }));
        }
        None
    }
}

pub trait Ligatures<'a> {
    fn ligatures(&self) -> Result<LigatureIter<'a>, IconResolutionError>;
    fn resolve_ligature(&self, name: &str) -> Result<GlyphId, IconResolutionError>;
}

impl<'a> Ligatures<'a> for FontRef<'a> {
    fn ligatures(&self) -> Result<LigatureIter<'a>, IconResolutionError> {
        ligatures(self, None)
    }
    fn resolve_ligature(&self, name: &str) -> Result<GlyphId, IconResolutionError> {
        let charmap = Charmap::new(self);
        let gids = name
            .chars()
            .map(|c| {
                charmap
                    .map(c)
                    .ok_or(IconResolutionError::UnmappedCharError(c))
            })
            .collect::<Result<Vec<_>, _>>()?;

        if gids.is_empty() {
            return Err(IconResolutionError::NoGlyphIds(name.to_string()));
        };

        ligatures(self, Some(gids))?
            .next()
            .ok_or(IconResolutionError::NoLigature(name.to_string()))?
            .map(|l| l.glyph)
    }
}

fn ligatures<'a>(
    font: &FontRef<'a>,
    gids: Option<Vec<GlyphId>>,
) -> Result<LigatureIter<'a>, IconResolutionError> {
    let lookups = font.gsub()?.lookup_list()?.lookups();
    let mut markers: LigatureInnerIter<'a> = Vec::new();
    for lookup in lookups.iter() {
        let SubstitutionSubtables::Ligature(table) = lookup?.subtables()? else {
            continue;
        };
        for table in table.iter() {
            let liga_subst = table?;
            let coverage = liga_subst.coverage()?;
            let liga_sets = liga_subst.ligature_sets();
            if let Some(ref gid) = gids {
                add_marker(&coverage, &liga_sets, gid[0], &mut markers)?;
            } else {
                for gid in coverage.iter() {
                    add_marker(&coverage, &liga_sets, gid, &mut markers)?;
                }
            }
        }
    }
    Ok(LigatureIter {
        inner_iter: markers,
        idx: 0,
        gids: gids.map(|mut f| f.split_off(1)),
    })
}

fn add_marker<'a>(
    coverage: &CoverageTable,
    liga_sets: &ArrayOfOffsets<'a, LigatureSet<'a>, Offset16>,
    gid: GlyphId,
    markers: &mut LigatureInnerIter<'a>,
) -> Result<(), ReadError> {
    if let Some(offset) = coverage.get(gid) {
        markers.push((
            gid,
            RefCell::new(Box::new(liga_sets.get(offset as usize)?.ligatures().iter())),
        ));
    };
    Ok(())
}
