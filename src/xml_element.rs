use tiny_skia::{Color, ColorU8};

/// A struct for constructing XML elements.
///
/// This struct allows building a tree of elements with attributes and children,
/// and then serializing it to a string using `std::fmt::Display`.
///
/// # Example
///
/// ```ignore
/// let rect = XmlElement::new("rect")
///     .with_attribute("x", 10)
///     .with_attribute("y", 10)
///     .with_attribute("width", 100)
///     .with_attribute("height", 100);
///
/// let svg = XmlElement::new("svg")
///     .with_attribute("width", 200)
///     .with_attribute("height", 200)
///     .with_child(rect);
///
/// println!("{}", svg);
/// ```
///
/// # TODO
///
/// Consider using another Xml builder implementation that can handle things like escapes.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct XmlElement {
    tag: String,
    attributes: Vec<(String, String)>,
    children: Vec<XmlElement>,
}

/// A color with hexadecimal string formatting.
pub struct HexColor(ColorU8);

impl From<Color> for HexColor {
    fn from(c: Color) -> HexColor {
        HexColor(c.to_color_u8())
    }
}

impl HexColor {
    /// Returns a new `HexColor` with the alpha channel set to opaque (255).
    pub fn opaque(self) -> HexColor {
        HexColor(ColorU8::from_rgba(
            self.0.red(),
            self.0.green(),
            self.0.blue(),
            u8::MAX,
        ))
    }
}

impl std::fmt::Display for HexColor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0.is_opaque() {
            write!(
                f,
                "#{:02x}{:02x}{:02x}",
                self.0.red(),
                self.0.green(),
                self.0.blue()
            )
        } else {
            write!(
                f,
                "#{:02x}{:02x}{:02x}{:02x}",
                self.0.red(),
                self.0.green(),
                self.0.blue(),
                self.0.alpha()
            )
        }
    }
}

/// A wrapper around `f64` that formats with at most 2 decimal places.
///
/// Trailing zeros and the decimal point are removed if they are not needed.
pub struct TruncatedFloat(pub f64);

impl From<f32> for TruncatedFloat {
    fn from(value: f32) -> Self {
        TruncatedFloat(value.into())
    }
}

impl std::fmt::Display for TruncatedFloat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut val = format!("{:.2}", self.0);
        if val.contains('.') {
            while val.ends_with('0') {
                val.pop();
            }
            if val.ends_with('.') {
                val.pop();
            }
        }
        write!(f, "{}", val)
    }
}

impl XmlElement {
    /// Creates a new `XmlElement` with the given tag name.
    pub fn new(tag: &str) -> Self {
        Self {
            tag: tag.to_string(),
            attributes: Vec::new(),
            children: Vec::new(),
        }
    }

    /// Adds an attribute to the element.
    pub fn add_attribute(&mut self, name: &str, value: impl ToString) {
        self.attributes.push((name.to_string(), value.to_string()));
    }

    /// Adds an attribute to the element.
    pub fn with_attribute(mut self, name: &str, value: impl ToString) -> Self {
        self.add_attribute(name, value);
        self
    }

    /// Adds a child element.
    pub fn add_child(&mut self, child: XmlElement) {
        self.children.push(child);
    }

    /// Adds a child element.
    pub fn with_child(mut self, child: XmlElement) -> Self {
        self.add_child(child);
        self
    }

    /// Adds children elements.
    pub fn add_children(&mut self, children: impl IntoIterator<Item = XmlElement>) {
        self.children.extend(children);
    }

    /// Adds children elements.
    pub fn with_children(mut self, children: impl IntoIterator<Item = XmlElement>) -> Self {
        self.add_children(children);
        self
    }
}

/// Formats the `XmlElement` as an XML string.
///
/// Supports the following formatting options:
/// - `{}`: Default formatting. Compact, single line for the element itself, recursive for children.
/// - `{:N}` (e.g., `{:2}`): Indented formatting. `N` specifies the number of spaces per indentation level.
/// - `{:#N}` (e.g., `{:#4}`): Also indent attributes, causing them to be formatted across multiple
///   lines (one per line) instead of all on the same line as the opening tag.
impl std::fmt::Display for XmlElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(width) = f.width() {
            self.fmt_with_indent(f, 0, width, f.alternate())
        } else {
            write!(f, "<{}", self.tag)?;
            for (name, value) in &self.attributes {
                write!(f, " {}=\"{}\"", name, value)?;
            }

            if self.children.is_empty() {
                write!(f, "/>")?;
            } else {
                write!(f, ">")?;
                for child in &self.children {
                    write!(f, "{}", child)?;
                }
                write!(f, "</{}>", self.tag)?;
            }
            Ok(())
        }
    }
}

impl XmlElement {
    fn fmt_with_indent(
        &self,
        f: &mut std::fmt::Formatter<'_>,
        indent_level: usize,
        indent_size: usize,
        alternate: bool,
    ) -> std::fmt::Result {
        let indent = " ".repeat(indent_level * indent_size);
        write!(f, "{}<{}", indent, self.tag)?;

        if alternate && !self.attributes.is_empty() {
            let attr_indent = " ".repeat((indent_level + 1) * indent_size);
            for (i, (name, value)) in self.attributes.iter().enumerate() {
                if i == 0 {
                    write!(f, " ")?;
                } else {
                    write!(f, "\n{}", attr_indent)?;
                }
                write!(f, "{}=\"{}\"", name, value)?;
            }
        } else {
            for (name, value) in &self.attributes {
                write!(f, " {}=\"{}\"", name, value)?;
            }
        }

        if self.children.is_empty() {
            write!(f, "/>")?;
        } else {
            writeln!(f, ">")?;
            for child in &self.children {
                child.fmt_with_indent(f, indent_level + 1, indent_size, alternate)?;
                writeln!(f)?;
            }
            write!(f, "{}</{}>", indent, self.tag)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_element() {
        let el = XmlElement::new("rect");
        assert_eq!(el.to_string(), "<rect/>");
    }

    #[test]
    fn element_with_attributes() {
        let el = XmlElement::new("rect")
            .with_attribute("x", 10)
            .with_attribute("y", "20");
        assert_eq!(el.to_string(), "<rect x=\"10\" y=\"20\"/>");
    }

    #[test]
    fn element_with_children() {
        let child = XmlElement::new("child");
        let parent = XmlElement::new("parent").with_child(child);
        assert_eq!(parent.to_string(), "<parent><child/></parent>");
    }

    #[test]
    fn nested_structure() {
        let child1 = XmlElement::new("child1").with_attribute("id", 1);
        let child2 = XmlElement::new("child2");
        let parent = XmlElement::new("parent")
            .with_attribute("name", "root")
            .with_child(child1)
            .with_child(child2);

        assert_eq!(
            parent.to_string(),
            "<parent name=\"root\"><child1 id=\"1\"/><child2/></parent>"
        );
    }

    #[test]
    fn format_indent() {
        let child = XmlElement::new("child").with_attribute("foo", "bar");
        let parent = XmlElement::new("parent").with_child(child);

        let output = format!("{:2}", parent);
        let expected = "<parent>\n  <child foo=\"bar\"/>\n</parent>";
        assert_eq!(output, expected);
    }

    #[test]
    fn format_alternate_indent() {
        let el = XmlElement::new("rect")
            .with_attribute("x", 10)
            .with_attribute("y", 20);

        let output = format!("{:#2}", el);
        let expected = "<rect x=\"10\"\n  y=\"20\"/>";
        assert_eq!(output, expected);
    }

    #[test]
    fn vector_tag_special_formatting() {
        let el = XmlElement::new("vector")
            .with_attribute(
                "xmlns:android",
                "http://schemas.android.com/apk/res/android",
            )
            .with_attribute("android:height", "24dp");

        let output = format!("{:#4}", el);

        let expected = "<vector xmlns:android=\"http://schemas.android.com/apk/res/android\"\n    android:height=\"24dp\"/>";
        assert_eq!(output, expected);
    }

    #[test]
    fn truncated_float_too_many_decimals_truncated_to_two() {
        assert_eq!(TruncatedFloat(1.23456).to_string(), "1.23");
    }

    #[test]
    fn truncated_float_one_decimal_remains_unchanged() {
        assert_eq!(TruncatedFloat(1.2).to_string(), "1.2");
    }

    #[test]
    fn truncated_float_whole_number_no_decimal_point() {
        assert_eq!(TruncatedFloat(1.0).to_string(), "1");
        assert_eq!(TruncatedFloat(100.00).to_string(), "100");
    }

    #[test]
    fn truncated_float_zero_is_just_zero() {
        assert_eq!(TruncatedFloat(0.0).to_string(), "0");
    }

    #[test]
    fn truncated_float_trailing_zero_removed() {
        assert_eq!(TruncatedFloat(100.500).to_string(), "100.5");
    }

    #[test]
    fn truncated_float_very_small_value_rounds_to_zero() {
        assert_eq!(TruncatedFloat(0.004).to_string(), "0");
    }

    #[test]
    fn truncated_float_small_value_rounds_up() {
        assert_eq!(TruncatedFloat(0.006).to_string(), "0.01");
    }

    #[test]
    fn hex_color_white_is_opaque_six_chars() {
        let white = HexColor::from(Color::WHITE);
        assert_eq!(white.to_string(), "#ffffff");
    }

    #[test]
    fn hex_color_black_is_opaque_six_chars() {
        let black = HexColor::from(Color::BLACK);
        assert_eq!(black.to_string(), "#000000");
    }

    #[test]
    fn hex_color_semi_transparent_is_eight_chars() {
        let transparent_red = HexColor::from(Color::from_rgba(1.0, 0.0, 0.0, 0.5).unwrap());
        assert_eq!(transparent_red.to_string(), "#ff000080");
    }

    #[test]
    fn hex_color_opaque_method_strips_alpha() {
        let transparent_red = HexColor::from(Color::from_rgba(1.0, 0.0, 0.0, 0.5).unwrap());
        let opaque_red = transparent_red.opaque();
        assert_eq!(opaque_red.to_string(), "#ff0000");
    }
}
