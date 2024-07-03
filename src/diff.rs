//! Diff 2 icon fonts to find out what got added,modified,removed.
//!

use crate::{
    error::IconResolutionError,
    iconid::{get_icons, Icon},
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
pub struct Diff {
    pub added: Vec<String>,
    pub modified: Vec<String>,
    pub removed: Vec<String>,
}

///
/// Compare 2 icon fonts `lhs`, `rhs` and returns names for
/// `Diff::added`: icons in `lhs` but not in `rhs`.
/// `Diff::modified`: icons in both `lhs` and `rhs` that draws differently.
/// `Diff::removed`: icons not in `lhs` but in `rhs`.
///
pub fn diff(lhs: &FontRef, rhs: &FontRef) -> Result<Diff, IconResolutionError> {
    let lhs_icons = get_icons(lhs)?;
    let rhs_icons = get_icons(rhs)?;
    let lhs_icons: HashMap<String, GlyphId> = map_by_names(lhs_icons);
    let rhs_icons: HashMap<String, GlyphId> = map_by_names(rhs_icons);
    let added = in_first_but_not_second(&lhs_icons, &rhs_icons);
    let removed = in_first_but_not_second(&rhs_icons, &lhs_icons);
    let modified = diff_glyphs(lhs_icons, rhs_icons, lhs, rhs)?;
    Ok(Diff {
        added,
        modified,
        removed,
    })
}

fn diff_glyphs(
    lhs_icons: HashMap<String, GlyphId>,
    rhs_icons: HashMap<String, GlyphId>,
    lhs: &FontRef,
    rhs: &FontRef,
) -> Result<Vec<String>, IconResolutionError> {
    let lhs_outlines = Tables::new(lhs)?;
    let rhs_outlines = Tables::new(rhs)?;
    let common: Vec<(String, GlyphId, GlyphId)> = lhs_icons
        .into_iter()
        .filter_map(|(k, v)| rhs_icons.get(&k).map(|r_gid| (k, v, *r_gid)))
        .collect();
    let result = common
        .par_iter()
        .map(|(name, lhs_gid, rhs_gid)| {
            let mut lhs_closure: Vec<_> = lhs
                .gsub()?
                .closure_glyphs([*lhs_gid].into())?
                .into_iter()
                .collect();
            let mut rhs_closure: Vec<_> = rhs
                .gsub()?
                .closure_glyphs([*rhs_gid].into())?
                .into_iter()
                .collect();
            if lhs_closure.len() != rhs_closure.len() {
                return Ok::<String, IconResolutionError>(name.to_string());
            }
            lhs_closure.sort();
            rhs_closure.sort();
            for (lhs_gid, rhs_gid) in lhs_closure.iter().zip(rhs_closure.iter()) {
                if !eq(&lhs_outlines, &rhs_outlines, *lhs_gid, *rhs_gid)? {
                    return Ok(name.to_string());
                }
            }
            Ok(String::from(""))
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|f| !f.is_empty());

    Ok(result.collect())
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
    lhs: &Tables,
    rhs: &Tables,
    lhs_gid: GlyphId,
    rhs_gid: GlyphId,
) -> Result<bool, IconResolutionError> {
    if lhs.gvar.is_some() ^ rhs.gvar.is_some() {
        return Err(IconResolutionError::Invalid(String::from(
            "To diff fonts, they both need to have the
            same type of glyph variation data (either both with gvar or both without).",
        )));
    }
    let l = lhs.outlines.get(lhs_gid).map(|f| draw_outlines(f));
    let r = rhs.outlines.get(rhs_gid).map(|f| draw_outlines(f));
    if l != r {
        return Ok(false);
    }

    if let (Some(gvar), Some(other_gvar)) = (&lhs.gvar, &rhs.gvar) {
        let (data, other_data) = (
            gvar.glyph_variation_data(lhs_gid)?,
            other_gvar.glyph_variation_data(rhs_gid)?,
        );
        let mut d1 = vec![];
        for t in data.tuples() {
            for d in t.deltas() {
                d1.push((d.position, d.x_delta, d.y_delta));
            }
        }
        let mut i = 0;
        for t in other_data.tuples() {
            for d in t.deltas() {
                if (d.position, d.x_delta, d.y_delta) != d1[i] {
                    return Ok(false);
                }
                i += 1;
            }
        }
        return Ok(d1.len() == i);
        // Compare intermediate_start and intermediate_end when https://github.com/googlefonts/fontations/pull/982 get released.
    }
    Ok(true)
}

fn draw_outlines(lhs: OutlineGlyph) -> BezPath {
    let mut lhs_pen = SvgPathPen::new();
    let _ = lhs.draw(
        DrawSettings::unhinted(Size::unscaled(), &Location::default()),
        &mut lhs_pen,
    );
    lhs_pen.into_inner()
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
        diff::{diff, Diff},
        testdata,
    };
    use std::time::Instant;

    #[test]
    fn diff_default() {
        let start_time = Instant::now();
        let font = FontRef::new(testdata::FULL_VF_OLD).unwrap();
        let new_font = FontRef::new(testdata::FULL_VF_NEW).unwrap();
        let expected = Diff {
            added: [
                "power_settings_circle",
                "rotate_auto",
                "convert_to_text",
                "multimodal_hand_eye",
                "voice_selection_off",
                "stack_hexagon",
                "sync_desktop",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            modified: [
                "flight_takeoff",
                "power_settings_new",
                "lock_reset",
                "flight_land",
                "airplanemode_inactive",
                "local_airport",
                "photo_prints",
                "flight",
                "connecting_airports",
                "airplanemode_active",
                "power_rounded",
                "travel",
                "flightsmode",
                "animated_images",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            removed: vec![],
        };

        let actual = diff(&new_font, &font).unwrap();

        assert_eq_diff(actual, expected);

        let elapsed_time = start_time.elapsed();

        println!("Elapsed time: {:.2?} seconds", elapsed_time);
    }

    #[test]
    fn same_fonts_empty_diff() {
        let start_time = Instant::now();
        let font = FontRef::new(testdata::FULL_VF_NEW).unwrap();
        let new_font = FontRef::new(testdata::FULL_VF_NEW).unwrap();
        let expected = Diff {
            added: vec![],
            modified: vec![],
            removed: vec![],
        };

        let actual = diff(&new_font, &font).unwrap();

        assert_eq_diff(actual, expected);

        let elapsed_time = start_time.elapsed();

        println!("Elapsed time: {:.2?} seconds", elapsed_time);
    }

    fn assert_eq_diff(actual: Diff, expected: Diff) {
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
