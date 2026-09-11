//! Minimal XML helpers: reading with .NET `DataContractSerializer` semantics, and writing.

use roxmltree::Node;

pub const XSI_NS: &str = "http://www.w3.org/2001/XMLSchema-instance";

/// Reads the members of a DataContract-serialized object the way .NET does: `members` must be in
/// serialization order (ordinal-sorted names), and elements are matched in that order. An element
/// naming a member before the last one matched is ignored, as is any unknown element.
///
/// Returns the matched (member index, element) pairs.
pub fn data_contract_members<'a, 'input>(node: Node<'a, 'input>, members: &[&str]) -> Vec<(usize, Node<'a, 'input>)> {
    let mut result = Vec::new();
    let mut current: Option<usize> = None;
    for child in node.children().filter(|c| c.is_element()) {
        let name = child.tag_name().name();
        let start = current.map_or(0, |c| c + 1);
        if let Some(offset) = members[start..].iter().position(|m| *m == name) {
            let j = start + offset;
            result.push((j, child));
            current = Some(j);
        }
    }
    result
}

/// True if an element is marked `i:nil="true"`.
pub fn is_nil(node: Node) -> bool {
    node.attribute((XSI_NS, "nil")) == Some("true")
}

/// All the text within an element.
pub fn text(node: Node) -> String {
    node.descendants().filter(|n| n.is_text()).filter_map(|n| n.text()).collect()
}

/// The first child element with a name.
pub fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children().find(|c| c.is_element() && c.tag_name().name() == name)
}

pub fn children<'a, 'input>(node: Node<'a, 'input>, name: &'a str) -> impl Iterator<Item = Node<'a, 'input>> + 'a {
    node.children().filter(move |c| c.is_element() && c.tag_name().name() == name)
}

/// A simple element tree for writing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct XElement {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub text: Option<String>,
    pub children: Vec<XElement>,
}

impl XElement {
    pub fn new(name: impl Into<String>) -> Self {
        XElement { name: name.into(), ..Default::default() }
    }

    pub fn with_text(name: impl Into<String>, text: impl Into<String>) -> Self {
        XElement { name: name.into(), text: Some(text.into()), ..Default::default() }
    }

    /// An element marked as null, DataContract style.
    pub fn nil(name: impl Into<String>) -> Self {
        XElement::new(name).attr("i:nil", "true")
    }

    pub fn attr(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.attrs.push((k.into(), v.into()));
        self
    }

    pub fn child(mut self, c: XElement) -> Self {
        self.children.push(c);
        self
    }

    pub fn push(&mut self, c: XElement) {
        self.children.push(c);
    }

    /// Serializes with two-space indentation (as .NET's `XDocument.Save` does).
    pub fn to_pretty_string(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, 0);
        out
    }

    fn write(&self, out: &mut String, depth: usize) {
        let indent = "  ".repeat(depth);
        out.push_str(&indent);
        out.push('<');
        out.push_str(&self.name);
        for (k, v) in &self.attrs {
            out.push_str(&format!(" {k}=\"{}\"", escape(v, true)));
        }
        let text = self.text.as_deref().unwrap_or("");
        if self.children.is_empty() && text.is_empty() {
            out.push_str(" />\n");
            return;
        }
        out.push('>');
        if self.children.is_empty() {
            out.push_str(&escape(text, false));
        } else {
            out.push('\n');
            for c in &self.children {
                c.write(out, depth + 1);
            }
            out.push_str(&indent);
        }
        out.push_str(&format!("</{}>\n", self.name));
    }
}

fn escape(s: &str, attribute: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attribute => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_order_matching_skips_out_of_order_elements() {
        let doc = roxmltree::Document::parse("<R><ID>x</ID><DisplayName>d</DisplayName><Slicing/><Junk/></R>").unwrap();
        let members = data_contract_members(doc.root_element(), &["DisplayName", "ID", "Slicing"]);
        let names: Vec<_> = members.iter().map(|(i, n)| (*i, n.tag_name().name())).collect();
        assert_eq!(names, vec![(1, "ID"), (2, "Slicing")]);
    }

    #[test]
    fn writes_and_escapes() {
        let e = XElement::new("A").attr("v", "1").child(XElement::with_text("B", "x<y & z")).child(XElement::new("C"));
        let s = e.to_pretty_string();
        assert_eq!(s, "<A v=\"1\">\n  <B>x&lt;y &amp; z</B>\n  <C />\n</A>\n");
        let doc = roxmltree::Document::parse(&s).unwrap();
        assert_eq!(text(child(doc.root_element(), "B").unwrap()), "x<y & z");
    }
}
