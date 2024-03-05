//! Identification of icons and resolution of glyph ids. Assumes Google style icon font input.
use skrifa::{
    charmap::Charmap,
    instance::LocationRef,
    raw::{
        tables::{
            gsub::{Gsub, LigatureSubstFormat1, SingleSubst, SubstitutionSubtables},
            layout::ConditionSet,
        },
        FontRef, ReadError, TableProvider, TopLevelTable,
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
            IconIdentifier::Name(name) => resolve_ligature(font, name.as_str()),
        }?;

        apply_location_based_substitution(font, location, gid)
            .map_err(IconResolutionError::ReadError)
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

/// gids is assumed non-empty
fn resolve_liga_subst(
    liga: &LigatureSubstFormat1<'_>,
    gids: &[GlyphId],
) -> Result<Option<GlyphId>, ReadError> {
    let first = gids[0];
    let coverage = liga.coverage()?;
    let Some(set_index) = coverage.get(first) else {
        return Ok(None);
    };
    let set = liga.ligature_sets().get(set_index as usize)?;
    // Seek a ligature that matches glyphs 2..N of name
    // We don't care about speed
    let gids = &gids[1..];
    for liga in set.ligatures().iter() {
        let liga = liga?;
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

/// gids is assumed non-empty
fn resolve_ligature_internal(
    font: &FontRef,
    gids: &[GlyphId],
) -> Result<Option<GlyphId>, ReadError> {
    // Try to find a ligature that starts with our first gid and then resolve against that
    // This is made uglier by the need to query extensions
    let gsub = font.gsub()?;
    let lookups = gsub.lookup_list()?;
    for lookup in lookups.lookups().iter() {
        let lookup = lookup?;
        let SubstitutionSubtables::Ligature(table) = lookup.subtables()? else {
            continue;
        };
        for liga in table.iter() {
            let liga = liga?;
            if let Some(gid) = resolve_liga_subst(&liga, gids)? {
                return Ok(Some(gid));
            }
        }
    }
    Ok(None)
}

pub fn resolve_ligature(font: &FontRef, name: &str) -> Result<GlyphId, IconResolutionError> {
    let charmap = Charmap::new(font);
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

    resolve_ligature_internal(font, &gids)
        .map_err(IconResolutionError::ReadError)?
        .ok_or_else(|| IconResolutionError::NoLigature(name.to_string()))
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

    use crate::{
        iconid::{LAN, MAIL, MAN},
        testdata_bytes,
    };

    use super::IconIdentifier;

    fn assert_gid_at<I>(identifier: &IconIdentifier, location: I, expected: GlyphId)
    where
        I: IntoIterator,
        I::Item: Into<VariationSetting>,
    {
        let raw_font = testdata_bytes("vf[FILL,GRAD,opsz,wght].ttf");
        let font = FontRef::new(&raw_font).unwrap();
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
}
