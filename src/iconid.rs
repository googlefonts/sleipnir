//! Identification of icons and resolution of glyph ids. Assumes Google style icon font input.
//!
use crate::error::IconResolutionError;
use crate::ligature::Ligatures;
use skrifa::{
    instance::LocationRef,
    raw::{
        tables::{
            gsub::{Gsub, SingleSubst, SubstitutionSubtables},
            layout::ConditionSet,
        },
        FontRef, ReadError, TableProvider, TopLevelTable,
    },
    GlyphId, MetadataProvider,
};
use smol_str::SmolStr;
use std::{
    collections::{
        hash_map::Entry::{Occupied, Vacant},
        HashMap,
    },
    ops::RangeInclusive,
};

// https://en.wikipedia.org/wiki/Private_Use_Areas
const _PUA_CODEPOINTS: [RangeInclusive<u32>; 3] =
    [0xE000..=0xF8FF, 0xF0000..=0xFFFFD, 0x100000..=0x10FFFD];

#[derive(Clone, Debug)]
pub enum IconIdentifier {
    GlyphId(GlyphId),
    Codepoint(u32),
    Name(SmolStr),
}

impl IconIdentifier {
    /// Until such time as we have memory safe shaping, simplified resolution of icons
    ///
    /// Resolves name => glyph id by seeking a ligature then applies singlesubst based on
    /// location in designspace. This is necessary and sufficient to do things like draw icon
    /// outlines for Google-style icon fonts.
    pub fn resolve(
        &self,
        font: &FontRef,
        location: &LocationRef,
    ) -> Result<GlyphId, IconResolutionError> {
        let gid = match self {
            IconIdentifier::GlyphId(gid) => Ok(*gid),
            IconIdentifier::Codepoint(cp) => font
                .cmap()
                .map_err(IconResolutionError::ReadError)?
                .map_codepoint(*cp)
                .ok_or(IconResolutionError::NoCmapEntry(*cp)),
            IconIdentifier::Name(name) => font.resolve_ligature(name.as_str()),
        }?;

        apply_location_based_substitution(font, location, gid)
            .map_err(IconResolutionError::ReadError)
    }
}

#[derive(Debug, PartialEq)]
pub struct Icon {
    name: String,
    codepoints: Vec<u32>,
    gid: GlyphId,
}

impl Icon {
    pub fn new(name: &str, codepoints: impl Into<Vec<u32>>, gid: u16) -> Self {
        Icon {
            name: String::from(name),
            codepoints: codepoints.into(),
            gid: GlyphId::new(gid),
        }
    }
}

fn matches(
    condition_set: Option<Result<ConditionSet<'_>, ReadError>>,
    location: &LocationRef,
) -> Result<bool, ReadError> {
    // See https://learn.microsoft.com/en-us/typography/opentype/spec/chapter2#featurevariations-table

    let Some(condition_set) = condition_set else {
        // If the ConditionSet offset is 0, there is no condition set table. This is treated as the universal condition: all contexts are matched.
        return Ok(true);
    };
    // For a given condition set, conditions are conjunctively related (boolean AND)
    let coords = location.coords();
    let condition_set = condition_set?;
    for condition in condition_set.conditions().iter() {
        let condition = condition?;
        let pos = coords
            .get(condition.axis_index() as usize)
            .map(|p| p.to_f32())
            .unwrap_or_default();
        let min = condition.filter_range_min_value().to_f32();
        let max = condition.filter_range_max_value().to_f32();
        if pos < min || pos > max {
            return Ok(false); // out of bounds
        }
    }
    Ok(true)
}

/// Pending availability of memory safe shaping apply single substitutions manually because the FILL
/// axis uses them to prevent seams that occur when shapes grow to be adjacent.
fn apply_location_based_substitution(
    font: &FontRef,
    location: &LocationRef,
    gid: GlyphId,
) -> Result<GlyphId, ReadError> {
    if font.table_data(Gsub::TAG).is_none() {
        return Ok(gid);
    }
    let gsub = font.gsub()?;
    let Some(feature_variations) = gsub.feature_variations() else {
        return Ok(gid);
    };

    let feature_variations = feature_variations?;
    let lookups = gsub.lookup_list()?;
    for record in feature_variations.feature_variation_records() {
        if !matches(
            record.condition_set(feature_variations.offset_data()),
            location,
        )? {
            continue;
        }

        let Some(feature_table_substitution) =
            record.feature_table_substitution(feature_variations.offset_data())
        else {
            // We found a live sub, it's a nop. Done.
            return Ok(gid);
        };
        let feature_table_substitution = feature_table_substitution?;

        for sub in feature_table_substitution.substitutions() {
            let alt = sub.alternate_feature(feature_table_substitution.offset_data())?;
            for lookup_idx in alt.lookup_list_indices() {
                let lookup = lookups.lookups().get(lookup_idx.get() as usize)?;
                let SubstitutionSubtables::Single(table) = lookup.subtables()? else {
                    continue;
                };
                for single in table.iter() {
                    let single = &single?;
                    let coverage = match single {
                        SingleSubst::Format1(single) => single.coverage()?,
                        SingleSubst::Format2(single) => single.coverage()?,
                    };
                    let Some(coverage_idx) = coverage.get(gid) else {
                        continue;
                    };
                    // This one is live
                    let new_gid = match single {
                        SingleSubst::Format1(single) => GlyphId::new(
                            (gid.to_u16() as i32 + single.delta_glyph_id() as i32) as u16,
                        ),
                        SingleSubst::Format2(single) => single
                            .substitute_glyph_ids()
                            .get(coverage_idx as usize)
                            .map(|be| be.get())
                            .unwrap_or(gid),
                    };
                    return Ok(new_gid);
                }
            }
        }
        // We need only apply the first live, supported, substitution
        break;
    }

    // If we got here there is no change
    Ok(gid)
}

/// Returns the ligatures of the given icon font.
/// Some assumptions are made:
/// each ligature glyph must have at least a single PUA codepoint assigned in cmap, if non-PUA is assigned, the ligature will be ignored.
/// each ligature component must have a single valid non-PUA codepoint entry in cmap.
/// a glyph is allowed to be assigned to multiple codepoints.
/// ligatures are allowed to point to the same glyph.
/// a glyph with a PUA and non-PUA codepoint is considered as single character ligature and will be returned in the result.
///
pub fn get_icons(font: &FontRef) -> Result<Vec<Icon>, IconResolutionError> {
    let charmap = font.charmap();
    let mut rev_non_pua_cmap: HashMap<GlyphId, u32> = HashMap::new();
    let mut rev_pua_cmap: HashMap<GlyphId, Vec<u32>> = HashMap::new();
    for (codepoint, gid) in charmap.mappings() {
        match is_pua(codepoint) {
            true => rev_pua_cmap.entry(gid).or_default().push(codepoint),
            false => {
                match rev_non_pua_cmap.entry(gid) {
                    Occupied(entry) => {
                        return Err(IconResolutionError::Invalid(format!(
                            "gid {} is assigned multiple non-PUA codepoints {},{}",
                            gid.to_u32(),
                            entry.get(),
                            codepoint
                        )))
                    }
                    Vacant(entry) => entry.insert(codepoint),
                };
            }
        };
    }

    // A glyph having both non-PUA and PUA codepoint is considered a single character ligature.
    let mut results: Vec<Icon> = rev_non_pua_cmap
        .iter()
        .filter(|(k, _)| rev_pua_cmap.contains_key(k))
        .map(|(k, c)| {
            Ok(Icon {
                name: String::from(
                    char::from_u32(*c).ok_or(IconResolutionError::InvalidCharacter(*c))?,
                ),
                codepoints: rev_pua_cmap.get(k).unwrap().clone(),
                gid: *k,
            })
        })
        .collect::<Result<Vec<Icon>, IconResolutionError>>()?;

    for ligature in font.ligatures()? {
        let ligature = ligature?;
        if rev_non_pua_cmap.contains_key(&ligature.glyph) {
            // while assiging non PUA to a liga is valid, we don't allow it.
            continue;
        }
        results.push(Icon {
            name: build_icon_name(ligature.components, &rev_non_pua_cmap)?,
            codepoints: rev_pua_cmap
                .get(&ligature.glyph)
                .ok_or_else(|| IconResolutionError::NoCmapEntryForGid(ligature.glyph.to_u32()))?
                .clone(),
            gid: ligature.glyph,
        });
    }

    Ok(results)
}

fn build_icon_name(
    gids: Vec<GlyphId>,
    rev_non_pua_cmap: &HashMap<GlyphId, u32>,
) -> Result<String, IconResolutionError> {
    Ok(gids
        .iter()
        .map(|gid| gid_to_char(gid, rev_non_pua_cmap))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .collect())
}

fn is_pua(codepoint: u32) -> bool {
    _PUA_CODEPOINTS.iter().any(|r| r.contains(&codepoint))
}

fn gid_to_char(
    gid: &GlyphId,
    rev_non_pua_cmap: &HashMap<GlyphId, u32>,
) -> Result<char, IconResolutionError> {
    let codepoint = *rev_non_pua_cmap
        .get(gid)
        .ok_or_else(|| IconResolutionError::NoCmapEntryForGid(gid.to_u32()))?;
    char::from_u32(codepoint).ok_or(IconResolutionError::InvalidCharacter(codepoint))
}

#[cfg(test)]
pub static MAIL: IconIdentifier = IconIdentifier::Codepoint(57688);
#[cfg(test)]
pub static LAN: IconIdentifier = IconIdentifier::Name(SmolStr::new_static("lan"));
#[cfg(test)]
pub static MAN: IconIdentifier = IconIdentifier::GlyphId(GlyphId::new(5));

#[cfg(test)]
mod tests {
    use skrifa::{setting::VariationSetting, FontRef, GlyphId, MetadataProvider};
    use write_fonts::{tables::cmap::Cmap, FontBuilder};

    use crate::{
        iconid::{get_icons, Icon, LAN, MAIL, MAN},
        testdata,
    };

    use super::IconIdentifier;

    fn assert_gid_at<I>(identifier: &IconIdentifier, location: I, expected: GlyphId)
    where
        I: IntoIterator,
        I::Item: Into<VariationSetting>,
    {
        let font = FontRef::new(testdata::ICON_FONT).unwrap();
        let location = font.axes().location(location);
        assert_eq!(
            expected,
            identifier.resolve(&font, &(&location).into()).unwrap()
        );
    }

    #[test]
    fn resolve_mail_icon_at_default() {
        assert_gid_at::<[(&str, f32); 0]>(&MAIL, [], GlyphId::new(1));
    }

    #[test]
    #[allow(non_snake_case)]
    fn resolve_mail_icon_at_FILL_0_98() {
        assert_gid_at(&MAIL, [("FILL", 0.98)], GlyphId::new(1));
    }

    #[test]
    #[allow(non_snake_case)]
    fn resolve_mail_icon_at_FILL_1() {
        assert_gid_at(&MAIL, [("FILL", 1.0)], GlyphId::new(2));
    }

    #[test]
    fn resolve_lan_icon_at_default() {
        assert_gid_at::<[(&str, f32); 0]>(&LAN, [], GlyphId::new(3));
    }

    #[test]
    #[allow(non_snake_case)]
    fn resolve_lan_icon_at_FILL_0_99() {
        assert_gid_at(&LAN, [("FILL", 0.99)], GlyphId::new(4));
    }

    #[test]
    fn resolve_man_icon_at_default() {
        assert_gid_at::<[(&str, f32); 0]>(&MAN, [], GlyphId::new(5));
    }

    #[test]
    fn get_icons_default() {
        let font_data = rebuild_font_with_cmap(
            testdata::LIGA_TESTS_FONT,
            |(_, _)| true,
            vec![('\u{E358}', GlyphId::new(3))],
        );

        let actual = get_icons(&FontRef::new(&font_data).unwrap());

        assert_eq!(
            actual.unwrap(),
            vec![
                Icon::new("x", [58180], 6),
                Icon::new("box_check", [58199, 58200], 3),
                Icon::new("news", [57394], 4),
                Icon::new("wrench", [59334], 5),
            ]
        );
    }

    #[test]
    fn get_icons_missing_component_cmap() {
        let font_data = rebuild_font_with_cmap(
            testdata::LIGA_TESTS_FONT,
            |(codepoint, _)| codepoint != &'b',
            vec![],
        );

        let actual = get_icons(&FontRef::new(&font_data).unwrap());

        actual.expect_err("Expected error for missing cmap entry");
    }

    #[test]
    fn get_icons_missing_ligature_cmap() {
        let font_data = rebuild_font_with_cmap(
            testdata::LIGA_TESTS_FONT,
            |(codepoint, _)| codepoint != &'\u{E357}',
            vec![],
        );

        let actual = get_icons(&FontRef::new(&font_data).unwrap());

        actual.expect_err("Expected error for missing cmap entry");
    }

    fn rebuild_font_with_cmap<T>(
        fontdata: &[u8],
        predicate: T,
        additional: Vec<(char, GlyphId)>,
    ) -> Vec<u8>
    where
        T: FnMut(&(char, GlyphId)) -> bool,
    {
        let font = FontRef::new(fontdata).unwrap();
        let new_cmap = Cmap::from_mappings(
            font.charmap()
                .mappings()
                .map(|(codepoint, glyph)| (std::char::from_u32(codepoint).unwrap(), glyph))
                .filter(predicate)
                .chain(additional)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        FontBuilder::new()
            .add_table(&new_cmap)
            .unwrap() // errors if we can't compile 'head', unlikely here
            .copy_missing_tables(font)
            .build()
    }
}
