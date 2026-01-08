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
pub struct XmlElement {
    tag: String,
    attributes: Vec<(String, String)>,
    children: Vec<XmlElement>,
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
}
