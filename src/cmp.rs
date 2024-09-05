//! Diff 2 icon fonts to find out what got added,modified,removed.
//!

use crate::{
    error::IconResolutionError,
    iconid::{Icon, Icons},
    pens::SvgPathPen,
};
use core::cmp::PartialEq;
use kurbo::BezPath;
use rayon::prelude::*;
use skrifa::{
    instance::{Location, Size},
    outline::DrawSettings,
    raw::{tables::gvar::Gvar, FontRef, ReadError, TableProvider},
    GlyphId, MetadataProvider, OutlineGlyph, OutlineGlyphCollection,
};
use std::collections::HashMap;

#[derive(Debug, PartialEq)]
pub struct CompareResult {
    /// Names of icons present in new but not old font.
    pub added: Vec<String>,
    /// Names of the icons present in both fonts but draws differently.
    pub modified: Vec<String>,
    /// Names of icons present in old but not new font.
    pub removed: Vec<String>,
}

/// Compares 2 icon fonts.
pub fn compare_fonts(old: &FontRef, new: &FontRef) -> Result<CompareResult, IconResolutionError> {
    let old_icons = old.icons()?;
    let new_icons = new.icons()?;
    let old_icons: HashMap<String, GlyphId> = map_by_names(old_icons);
    let new_icons: HashMap<String, GlyphId> = map_by_names(new_icons);
    let added = in_first_but_not_second(&new_icons, &old_icons);
    let removed = in_first_but_not_second(&old_icons, &new_icons);
    let modified = diff_glyphs(old_icons, new_icons, old, new)?;
    Ok(CompareResult {
        added,
        modified,
        removed,
    })
}

fn diff_glyphs(
    old_icons: HashMap<String, GlyphId>,
    new_icons: HashMap<String, GlyphId>,
    old: &FontRef,
    new: &FontRef,
) -> Result<Vec<String>, IconResolutionError> {
    let old_outlines = Tables::new(old)?;
    let new_outlines = Tables::new(new)?;
    // Icons exist in both fonts.
    let common: Vec<(String, GlyphId, GlyphId)> = old_icons
        .into_iter()
        .filter_map(|(k, v)| new_icons.get(&k).map(|r_gid| (k, v, *r_gid)))
        .collect();
    Ok(common
        .par_iter()
        // Returns the names of modified icons, or None.
        .map(|(name, old_gid, new_gid)| {
            let mut old_closure: Vec<_> = old
                .gsub()?
                .closure_glyphs([*old_gid].into())?
                .into_iter()
                .collect();
            let mut new_closure: Vec<_> = new
                .gsub()?
                .closure_glyphs([*new_gid].into())?
                .into_iter()
                .collect();
            if old_closure.len() != new_closure.len() {
                // If closure changed assume the icon is modified.
                return Ok::<Option<String>, IconResolutionError>(Some(name.to_string()));
            }
            old_closure.sort();
            new_closure.sort();
            for (old_gid, new_gid) in old_closure.iter().zip(new_closure.iter()) {
                if !eq(&old_outlines, &new_outlines, *old_gid, *new_gid)? {
                    // Icon draws differently.
                    return Ok(Some(name.to_string()));
                }
            }
            // Icons draw glyphs are equal.
            Ok(None)
        })
        // Report back any error.
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect())
}

struct Tables<'a> {
    gvar: Option<Gvar<'a>>,
    outlines: OutlineGlyphCollection<'a>,
}

impl<'a> Tables<'a> {
    fn new(font: &'a FontRef) -> Result<Tables<'a>, ReadError> {
        Ok(Tables {
            gvar: font.gvar().ok(),
            outlines: font.outline_glyphs(),
        })
    }
}

fn eq(
    old: &Tables,
    new: &Tables,
    old_gid: GlyphId,
    new_gid: GlyphId,
) -> Result<bool, IconResolutionError> {
    if old.gvar.is_some() != new.gvar.is_some() {
        return Err(IconResolutionError::Invalid(String::from(
            "To diff fonts, they both need to have the
            same type of glyph variation data (either both with gvar or both without).",
        )));
    }
    let l = old.outlines.get(old_gid).map(|f| draw_outline(f));
    let r = new.outlines.get(new_gid).map(|f| draw_outline(f));
    if l != r {
        return Ok(false);
    }

    if let (Some(gvar), Some(other_gvar)) = (&old.gvar, &new.gvar) {
        let (data, other_data) = (
            gvar.glyph_variation_data(old_gid)?,
            other_gvar.glyph_variation_data(new_gid)?,
        );
        let mut tuples = data.tuples();
        let mut other_tuples = other_data.tuples();
        loop {
            match (tuples.next(), other_tuples.next()) {
                // we have an item from both tuple lists
                (Some(tuple), Some(other_tuple)) => {
                    // note: iterators have eq() and ne() methods that work when
                    // the item impls PartialEq
                    if tuple.peak() != other_tuple.peak() || tuple.deltas().ne(other_tuple.deltas())
                    {
                        return Ok(false);
                    }
                }
                // we've reached the end of both lists
                (None, None) => break,
                // the lists were different sizes
                _ => return Ok(false),
            }
        }
        // Compare intermediate_start and intermediate_end when https://github.com/googlefonts/fontations/pull/982 get released.
    }
    Ok(true)
}

fn draw_outline(old: OutlineGlyph) -> BezPath {
    let mut old_pen = SvgPathPen::new();
    let _ = old.draw(
        DrawSettings::unhinted(Size::unscaled(), &Location::default()),
        &mut old_pen,
    );
    old_pen.into_inner()
}

fn map_by_names(icons: Vec<Icon>) -> HashMap<String, GlyphId> {
    icons
        .into_iter()
        .flat_map(|i| i.names.into_iter().map(move |n| (n, i.gid)))
        .collect()
}

fn in_first_but_not_second(
    first: &HashMap<String, GlyphId>,
    second: &HashMap<String, GlyphId>,
) -> Vec<String> {
    first
        .keys()
        .filter(|k| !second.contains_key(*k))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use skrifa::FontRef;

    use crate::{
        cmp::{compare_fonts, CompareResult},
        testdata,
    };
    use std::time::Instant;

    #[test]
    fn compare_fonts_default() {
        let start_time = Instant::now();
        let font = FontRef::new(testdata::FULL_VF_OLD).unwrap();
        let new_font = FontRef::new(testdata::FULL_VF_NEW).unwrap();
        let expected = CompareResult {
            added: vec!["settings".to_string()],
            modified: vec![
                "all_match".to_string(),
                "backspace".to_string(),
                "label".to_string(),
            ],
            removed: vec!["menu".to_string()],
        };

        let actual = compare_fonts(&font, &new_font).unwrap();
        assert_eq_diff(actual, expected);

        let elapsed_time = start_time.elapsed();

        println!("Elapsed time: {:.2?} seconds", elapsed_time);
    }

    #[test]
    fn compare_fonts_same_fonts_empty_diff() {
        let start_time = Instant::now();
        let font = FontRef::new(testdata::FULL_VF_NEW).unwrap();
        let new_font = FontRef::new(testdata::FULL_VF_NEW).unwrap();
        let expected = CompareResult {
            added: vec![],
            modified: vec![],
            removed: vec![],
        };

        let actual = compare_fonts(&new_font, &font).unwrap();

        assert_eq_diff(actual, expected);

        let elapsed_time = start_time.elapsed();

        println!("Elapsed time: {:.2?} seconds", elapsed_time);
    }

    fn assert_eq_diff(actual: CompareResult, expected: CompareResult) {
        assert_eq_vec(&actual.added, &expected.added);
        assert_eq_vec(&actual.modified, &expected.modified);
        assert_eq_vec(&actual.removed, &expected.removed);
    }

    fn assert_eq_vec(actual: &[String], expected: &[String]) {
        // assert_matches! is marked unstable, for now, workaround.
        assert!(expected.iter().all(|item| actual.contains(item)));
        assert_eq!(actual.len(), expected.len());
    }
}
