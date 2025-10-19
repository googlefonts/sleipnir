//! Converts a font to an SVG font with some limitations.
// It doesn't rely on a shaping engine, glyphs for isolated(isol) form are not extracted or
// generated.
// Only glyphs within the hardcoded Unicode ranges are included. Glyphs outside these ranges are
// omitted.
// Contextual Substitutions: Only supports single substitution type 1. More advanced GSUB lookups
// like contextual or chaining contextual substitutions (Types 5-8) are not supported.
// GPOS Kerning: The code reads the legacy kern table but does not read pair adjustment positioning
// data from the GPOS table, which is the more modern way to store kerning.
// Mark Attachment: There is no handling of GPOS features for mark-to-base or mark-to-mark
// positioning (GPOS Lookup Types 4-6), which is essential for placing accents and diacritics
// correctly in many scripts (e.g., Arabic vowel marks).
// Full Glyph Coverage: Only glyphs within the hardcoded Unicode ranges are included. Glyphs
// outside these ranges are omitted.
// Vertical Metrics: No support for vertical layout metrics or vertical kerning (<vkern>).
// Variable Font Instances: If the source font is a variable font, only the default instance's
// outlines and metrics are used. Variations are not supported.
use kurbo::Affine;
use skrifa::{
    instance::{LocationRef, Size},
    outline::DrawSettings,
    raw::{
        tables::{
            gsub::{ExtensionSubstFormat1, Gsub, SingleSubst, SubstitutionSubtables},
            kern::{self},
            layout::Subtables,
        },
        FontRef, TableProvider,
    },
    GlyphId, MetadataProvider, Tag,
};
use std::collections::HashMap;
use std::fmt::Write;
use std::result::Result;

use crate::{pathstyle::SvgPathStyle, pens::SvgPathPen};

const ARAB_SCRIPT_TAG: Tag = Tag::new(b"arab");
const INIT_FEATURE_TAG: Tag = Tag::new(b"init");
const MEDI_FEATURE_TAG: Tag = Tag::new(b"medi");
const FINA_FEATURE_TAG: Tag = Tag::new(b"fina");

/// Generates an SVG font from the given font data.
pub fn generate_svg_font(font: &FontRef, font_id: &str) -> Result<Vec<u8>, std::fmt::Error> {
    let mut svg_string = String::new();

    write_svg_header(&mut svg_string)?;
    write_font_element_start(&mut svg_string, font, font_id)?;

    let gsub_subs = GsubSubs::new(font);
    let charmap = font.charmap();

    let ranges = [
        (0x0020, 0x007e),
        (0x00a0, 0x00ff),
        (0x2013, 0x2013),
        (0x2014, 0x2014),
        (0x2018, 0x2018),
        (0x2019, 0x2019),
        (0x201a, 0x201a),
        (0x201c, 0x201c),
        (0x201d, 0x201d),
        (0x201e, 0x201e),
        (0x2022, 0x2022),
        (0x2039, 0x2039),
        (0x203a, 0x203a),
    ];

    for (start, end) in ranges {
        for codepoint in start..=end {
            if let Some(glyph_id) = charmap.map(codepoint) {
                if glyph_id.to_u32() == 0 {
                    continue;
                }
                write_glyph(&mut svg_string, font, codepoint, glyph_id, &gsub_subs)?;
            }
        }
    }

    write_kerning(&mut svg_string, font)?;
    write_font_element_end(&mut svg_string)?;
    write_svg_footer(&mut svg_string)?;

    Ok(svg_string.into_bytes())
}

fn get_panose_str(font: &FontRef) -> Option<String> {
    font.table_data(Tag::new(b"OS/2")).and_then(|data| {
        if data.as_bytes().len() >= 42 {
            let panose_bytes = &data.as_bytes()[32..42];
            let panose_str = panose_bytes
                .iter()
                .map(|b| b.to_string())
                .collect::<Vec<_>>()
                .join(" ");
            Some(panose_str)
        } else {
            None
        }
    })
}

fn write_svg_header(svg: &mut String) -> Result<(), std::fmt::Error> {
    writeln!(svg, "<?xml version=\"1.0\" standalone=\"no\"?>")?;
    writeln!(
        svg,
        "<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\">"
    )?;
    writeln!(svg, "<svg xmlns=\"http://www.w3.org/2000/svg\">")?;
    writeln!(svg, "  <defs>")?;
    Ok(())
}

fn write_font_element_start(
    svg: &mut String,
    font: &FontRef,
    id: &str,
) -> Result<(), std::fmt::Error> {
    let metrics = font.metrics(Size::unscaled(), LocationRef::default());
    let avg_char_width = font.os2().map(|os2| os2.x_avg_char_width()).unwrap_or(0);
    let units_per_em = metrics.units_per_em;
    let ascent = metrics.ascent;
    let descent = metrics.descent;

    writeln!(
        svg,
        "    <font id=\"{id}\" horiz-adv-x=\"{avg_char_width}\">"
    )?;
    let font_family = font
        .localized_strings(skrifa::string::StringId::FAMILY_NAME)
        .english_or_first()
        .map(|s| s.to_string())
        .unwrap_or_else(|| id.to_string());
    let mut font_face =
        format!("      <font-face font-family=\"{font_family}\" units-per-em=\"{units_per_em}\"");
    if let Some(panose_str) = get_panose_str(font) {
        write!(&mut font_face, " panose-1=\"{panose_str}\"").unwrap();
    }
    write!(
        &mut font_face,
        " ascent=\"{ascent}\" descent=\"{descent}\" alphabetic=\"0\" />"
    )
    .unwrap();
    writeln!(svg, "{}", font_face)?;
    Ok(())
}

fn write_glyph(
    svg: &mut String,
    font: &FontRef,
    codepoint: u32,
    glyph_id: GlyphId,
    gsub_subs: &GsubSubs,
) -> Result<(), std::fmt::Error> {
    let glyph_name_map = font.glyph_names();
    let glyph_name = glyph_name_map
        .get(glyph_id)
        .map(|n| n.as_str().to_string())
        .unwrap_or_default();
    let advance_width = font
        .glyph_metrics(Size::unscaled(), LocationRef::default())
        .advance_width(glyph_id)
        .unwrap_or_default();
    let mut path_d_attr = String::new();
    let mut pen = SvgPathPen::new_with_transform(Affine::IDENTITY);
    if let Some(outline) = font.outline_glyphs().get(glyph_id) {
        let _ = outline.draw(
            DrawSettings::unhinted(Size::unscaled(), LocationRef::default()),
            &mut pen,
        );
    }
    let path = pen.into_inner();
    if !path.elements().is_empty() {
        path_d_attr = format!(" d=\"{}\"", SvgPathStyle::Compact.write_svg_path(&path));
    }

    let escaped_codepoint = match char::from_u32(codepoint) {
        Some('\'') => "&apos;".to_string(),
        Some('\"') => "&quot;".to_string(),
        Some('&') => "&amp;".to_string(),
        Some('<') => "&lt;".to_string(),
        Some('>') => "&gt;".to_string(),
        Some(c) if (' '..='~').contains(&c) => c.to_string(),
        _ => format!("&#x{:x};", codepoint),
    };

    writeln!(
        svg,
        "      <glyph unicode=\"{}\" glyph-name=\"{}\" horiz-adv-x=\"{}\"{} />",
        escaped_codepoint, glyph_name, advance_width, path_d_attr
    )?;

    if let Some(sub_gid) = gsub_subs.init.get(&glyph_id) {
        if sub_gid.to_u32() != 0 {
            write_subst_glyph(svg, font, *sub_gid, "initial", &escaped_codepoint)?;
        }
    }
    if let Some(sub_gid) = gsub_subs.medi.get(&glyph_id) {
        if sub_gid.to_u32() != 0 {
            write_subst_glyph(svg, font, *sub_gid, "medial", &escaped_codepoint)?;
        }
    }
    if let Some(sub_gid) = gsub_subs.fina.get(&glyph_id) {
        if sub_gid.to_u32() != 0 {
            write_subst_glyph(svg, font, *sub_gid, "terminal", &escaped_codepoint)?;
        }
    }
    Ok(())
}

fn write_subst_glyph(
    svg: &mut String,
    font: &FontRef,
    subst_gid: GlyphId,
    arabic_form: &str,
    codepoint: &str,
) -> Result<(), std::fmt::Error> {
    let glyph_name_map = font.glyph_names();
    let glyph_name = glyph_name_map
        .get(subst_gid)
        .map(|n| n.as_str().to_string())
        .unwrap_or_default();
    let advance_width = font
        .glyph_metrics(Size::unscaled(), LocationRef::default())
        .advance_width(subst_gid)
        .unwrap_or_default();
    let mut path_d_attr = String::new();
    let mut pen = SvgPathPen::new_with_transform(Affine::IDENTITY);
    if let Some(outline) = font.outline_glyphs().get(subst_gid) {
        let _ = outline.draw(
            DrawSettings::unhinted(Size::unscaled(), LocationRef::default()),
            &mut pen,
        );
    }
    let path = pen.into_inner();
    if !path.elements().is_empty() {
        path_d_attr = format!(" d=\"{}\"", SvgPathStyle::Compact.write_svg_path(&path));
    }
    writeln!(
        svg,
        "      <glyph unicode=\"{}\" glyph-name=\"{}\" horiz-adv-x=\"{}\"{} arabic-form=\"{}\" />",
        codepoint, glyph_name, advance_width, path_d_attr, arabic_form,
    )?;
    Ok(())
}

fn write_kerning(svg: &mut String, font: &FontRef) -> Result<(), std::fmt::Error> {
    let glyph_names = font.glyph_names();
    if let Ok(kern) = font.kern() {
        if let Some(Ok(subtable)) = kern.subtables().next() {
            if let Ok(kern::SubtableKind::Format0(format0)) = subtable.kind() {
                for pair in format0.pairs() {
                    let left_glyph_id: GlyphId = pair.left().into();
                    let right_glyph_id: GlyphId = pair.right().into();
                    let g1 = glyph_names.get(left_glyph_id);
                    let g2 = glyph_names.get(right_glyph_id);
                    if g1.is_some() && g2.is_some() {
                        writeln!(
                            svg,
                            "      <hkern g1=\"{}\" g2=\"{}\" k=\"{}\" />",
                            g1.unwrap().as_str(),
                            g2.unwrap().as_str(),
                            -pair.value()
                        )?;
                    } else {
                        writeln!(
                            svg,
                            "      <hkern u1=\"{}\" u2=\"{}\" k=\"{}\" />",
                            pair.left().to_u16(),
                            pair.right().to_u16(),
                            -pair.value()
                        )?;
                    }
                }
            }
        }
    }
    Ok(())
}

fn write_font_element_end(svg: &mut String) -> Result<(), std::fmt::Error> {
    writeln!(svg, "    </font>")?;
    Ok(())
}

fn write_svg_footer(svg: &mut String) -> Result<(), std::fmt::Error> {
    writeln!(svg, "  </defs>")?;
    writeln!(svg, "</svg>")?;
    Ok(())
}

struct GsubSubs {
    init: HashMap<GlyphId, GlyphId>,
    medi: HashMap<GlyphId, GlyphId>,
    fina: HashMap<GlyphId, GlyphId>,
}

impl GsubSubs {
    fn new(font: &FontRef) -> Self {
        let mut gsub_subs = GsubSubs {
            init: HashMap::new(),
            medi: HashMap::new(),
            fina: HashMap::new(),
        };
        if let Ok(gsub) = font.gsub() {
            gsub_subs.populate(&gsub);
        }
        gsub_subs
    }

    fn populate(&mut self, gsub: &Gsub) {
        self.init = get_subst_map(gsub, INIT_FEATURE_TAG).unwrap_or_default();
        self.medi = get_subst_map(gsub, MEDI_FEATURE_TAG).unwrap_or_default();
        self.fina = get_subst_map(gsub, FINA_FEATURE_TAG).unwrap_or_default();
    }
}

fn get_subst_map(gsub: &Gsub, feature_tag: Tag) -> Option<HashMap<GlyphId, GlyphId>> {
    let script_list = gsub.script_list().ok()?;
    let feature_list = gsub.feature_list().ok()?;
    let lookup_list = gsub.lookup_list().ok()?;

    let script = script_list
        .script_records()
        .iter()
        .find(|sr| sr.script_tag() == ARAB_SCRIPT_TAG)
        .and_then(|sr| sr.script(script_list.offset_data()).ok())?;
    let langsys = script.default_lang_sys()?.ok()?;

    langsys.feature_indices().iter().find_map(|feature_idx| {
        let feature_rec = feature_list
            .feature_records()
            .get(feature_idx.get() as usize)?;
        if feature_rec.feature_tag() != feature_tag {
            return None;
        }
        let feature = feature_rec.feature(feature_list.offset_data()).ok()?;
        // We only consider features with at least one lookup, and only use the first lookup
        let lookup_idx = feature.lookup_list_indices().first()?;
        let lookup = lookup_list.lookups().get(lookup_idx.get() as usize).ok()?;
        if lookup.lookup_type() == 1 {
            // Single substitution
            if let Ok(SubstitutionSubtables::Single(subtables)) = lookup.subtables() {
                return collect_single_substitutions(subtables);
            }
        }
        None
    })
}

fn collect_single_substitutions<'a>(
    subtables: Subtables<'a, SingleSubst<'a>, ExtensionSubstFormat1<'a, SingleSubst<'a>>>,
) -> Option<HashMap<GlyphId, GlyphId>> {
    let mut map = HashMap::new();
    for subtable in subtables.iter().filter_map(|st| st.ok()) {
        match subtable {
            SingleSubst::Format1(table) => {
                if let Ok(coverage) = table.coverage() {
                    for glyph_id in coverage.iter() {
                        map.insert(
                            glyph_id.into(),
                            GlyphId::new(
                                (glyph_id.to_u16() as i32 + table.delta_glyph_id() as i32) as u16
                                    as u32,
                            ),
                        );
                    }
                }
            }
            SingleSubst::Format2(table) => {
                if let Ok(coverage) = table.coverage() {
                    for (glyph_id, subst_glyph_id) in
                        coverage.iter().zip(table.substitute_glyph_ids())
                    {
                        map.insert(glyph_id.into(), subst_glyph_id.get().into());
                    }
                }
            }
        }
    }
    Some(map)
}
