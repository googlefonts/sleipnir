use harfrust::{FontRef, GlyphBuffer, ShaperData, UnicodeBuffer};
use skrifa::{
    prelude::{LocationRef, Size},
    FontRef as SkrifaFontRef, MetadataProvider,
};

// TODO: add Location (aka VF settings) or DrawOptions without identifier
pub fn shape(text: &str, font: &FontRef) -> GlyphBuffer {
    let data = ShaperData::new(font);
    let shaper = data.shaper(font).build();

    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.guess_segment_properties();

    shaper.shape(buffer, &[])
}

fn get_text_width(text: &str, font: &FontRef, skrifa_font: &SkrifaFontRef, font_size: f32) -> f32 {
    let glyphs = shape(text, font);
    let upem = skrifa_font
        .metrics(Size::unscaled(), LocationRef::default())
        .units_per_em as f32;
    let scale = font_size / upem;
    glyphs
        .glyph_positions()
        .iter()
        .map(|pos| pos.x_advance)
        .sum::<i32>() as f32
        * scale
}

/// Calculates the height that text would take up in a given font.
///
/// # Arguments
///
/// * `text`: The text to measure.
/// * `font_size`: The font size in pixels.
/// * `line_spacing`: The line spacing relative to the font size.
/// * `width`: A maximum width constraint for the text layout in pixels.
/// * `font_bytes`: The font file bytes.
///
/// # Returns
///
/// The height of the text in pixels.
pub fn measure_height_px(
    text: String,
    font_size: f32,
    line_spacing: f32,
    width: f32,
    font_bytes: &[u8],
) -> Result<f32, Box<dyn std::error::Error>> {
    let harf_font_ref = FontRef::new(font_bytes).expect("For font files to be font files!");
    let skrifa_font_ref = SkrifaFontRef::new(font_bytes).expect("Fonts to be fonts");

    let metrics = skrifa_font_ref.metrics(Size::new(font_size), LocationRef::default());
    let line_height = (metrics.ascent - metrics.descent + metrics.leading) * line_spacing;

    let mut all_lines = Vec::new();
    for text_line in text.lines() {
        let mut lines = Vec::new();
        let mut current_line = String::new();

        // TODO: splitting whitespace may not be right for \t or other special characters.
        for word in text_line.split_whitespace() {
            let potential_line = if current_line.is_empty() {
                word.to_string()
            } else {
                format!("{} {}", current_line, word)
            };

            if get_text_width(&potential_line, &harf_font_ref, &skrifa_font_ref, font_size) <= width
            {
                current_line = potential_line;
            } else {
                let should_break_word = current_line.is_empty() || potential_line.contains(" ");

                if !current_line.is_empty() {
                    lines.push(current_line);
                }

                if should_break_word
                    && get_text_width(word, &harf_font_ref, &skrifa_font_ref, font_size) > width
                {
                    let mut temp_word = String::new();
                    for c in word.chars() {
                        let next_temp_word = format!("{}{}", temp_word, c);
                        if !temp_word.is_empty()
                            && get_text_width(
                                &next_temp_word,
                                &harf_font_ref,
                                &skrifa_font_ref,
                                font_size,
                            ) > width
                        {
                            lines.push(temp_word);
                            temp_word = c.to_string();
                        } else {
                            temp_word = next_temp_word;
                        }
                    }
                    current_line = temp_word;
                } else {
                    current_line = word.to_string();
                }
            }
        }
        if !current_line.is_empty() {
            lines.push(current_line);
        }
        all_lines.extend(lines);
    }

    let total_height = all_lines.len() as f32 * line_height;

    Ok(total_height)
}

#[cfg(test)]
mod tests {
    use crate::{measure::measure_height_px, testdata};

    #[test]
    fn single_line_height() {
        let text = "Hello";
        let font_size = 16.0;
        let line_spacing = 1.33;
        let width = 100.0;

        let actual_height = measure_height_px(
            text.to_string(),
            font_size,
            line_spacing,
            width,
            testdata::ICON_FONT,
        )
        .unwrap();
        let expected_height = 25.536001f32;
        assert_eq!(
            actual_height, expected_height,
            "Expected\n{expected_height}\n!= Actual\n{actual_height}",
        );
    }

    #[test]
    fn two_lines_height() {
        let text = "Hello\nWorld!";
        let font_size = 16.0;
        let line_spacing = 1.33;
        let width = 100.0;

        let actual_height = measure_height_px(
            text.to_string(),
            font_size,
            line_spacing,
            width,
            testdata::ICON_FONT,
        )
        .unwrap();
        let expected_height = 51.072002f32;
        assert_eq!(
            actual_height, expected_height,
            "Expected\n{expected_height}\n!= Actual\n{actual_height}",
        );
    }

    #[test]
    fn multiple_lines_with_word_breaking_height() {
        let text = "Hello Looooooooooooooong World and some";
        let font_size = 16.0;
        let line_spacing = 1.33;
        let width = 100.0;

        let actual_height = measure_height_px(
            text.to_string(),
            font_size,
            line_spacing,
            width,
            testdata::ICON_FONT,
        )
        .unwrap();
        let expected_height = 178.75201f32;
        assert_eq!(
            actual_height, expected_height,
            "Expected\n{expected_height}\n!= Actual\n{actual_height}",
        );
    }
}
