//! YAML frontmatter, emitted and parsed without a YAML dependency.
//!
//! The frontmatter here is a flat map of scalars and string lists — enough to
//! carry a card's identity and let editors and note tools index it, and far
//! short of what would justify pulling in a YAML parser and its transitive
//! dependencies into a binary with a sub-10ms startup budget.
//!
//! Values are quoted whenever they could otherwise be misread as YAML syntax,
//! which matters because symbol names legitimately contain `:` (Rust paths),
//! `#` (anchors), and leading `*` or `&` (pointer and reference types).

use std::collections::BTreeMap;

pub const FENCE: &str = "---";

/// An ordered set of frontmatter fields.
///
/// `BTreeMap` rather than insertion order: the file gets committed, so two
/// machines must produce byte-identical output.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Frontmatter {
    scalars: BTreeMap<String, String>,
    lists: BTreeMap<String, Vec<String>>,
}

impl Frontmatter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, key: &str, value: impl Into<String>) -> &mut Self {
        self.scalars.insert(key.to_string(), value.into());
        self
    }

    pub fn set_list(&mut self, key: &str, values: Vec<String>) -> &mut Self {
        self.lists.insert(key.to_string(), values);
        self
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.scalars.get(key).map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.scalars.is_empty() && self.lists.is_empty()
    }

    /// Render as a fenced YAML block, or an empty string when there is nothing
    /// to say. An empty `---\n---` block is noise that some renderers show as a
    /// horizontal rule.
    pub fn render(&self) -> String {
        if self.is_empty() {
            return String::new();
        }

        let mut out = String::from(FENCE);
        out.push('\n');

        for (key, value) in &self.scalars {
            out.push_str(&format!("{key}: {}\n", quote(value)));
        }

        for (key, values) in &self.lists {
            if values.is_empty() {
                out.push_str(&format!("{key}: []\n"));
                continue;
            }

            out.push_str(&format!("{key}:\n"));
            for value in values {
                out.push_str(&format!("  - {}\n", quote(value)));
            }
        }

        out.push_str(FENCE);
        out.push('\n');
        out
    }
}

/// Quote a scalar when leaving it bare would change its meaning.
fn quote(value: &str) -> String {
    let needs_quoting = value.is_empty()
        || value.starts_with(' ')
        || value.ends_with(' ')
        || value.contains(": ")
        || value.ends_with(':')
        // A quote mid-scalar is legal YAML, but quoting anyway keeps the
        // output unambiguous for the many tools that parse frontmatter with a
        // regex rather than a real parser.
        || value.contains(['"', '\''])
        || value.starts_with(['#', '&', '*', '!', '|', '>', '%', '@', '`', '"', '\'', '[', '{', '-', '?'])
        || value.contains('\n')
        // Bare `true`, `null`, and numbers would parse as non-strings.
        || matches!(
            value.to_ascii_lowercase().as_str(),
            "true" | "false" | "null" | "yes" | "no" | "on" | "off" | "~"
        )
        || value.parse::<f64>().is_ok();

    if !needs_quoting {
        return value.to_string();
    }

    let escaped = value
        .replace('\\', r"\\")
        .replace('"', r#"\""#)
        .replace('\n', r"\n");
    format!("\"{escaped}\"")
}

/// Split a document into its frontmatter block and its body.
///
/// Returns `(None, whole_document)` when there is no frontmatter, so callers
/// can treat an unfenced file as body-only rather than special-casing it.
pub fn split(document: &str) -> (Option<&str>, &str) {
    let Some(rest) = document.strip_prefix(FENCE) else {
        return (None, document);
    };
    let rest = rest.strip_prefix('\n').unwrap_or(rest);

    let Some(end) = rest.find(&format!("\n{FENCE}")) else {
        // An opening fence with no closing one is not frontmatter; treating it
        // as such would swallow the whole file.
        return (None, document);
    };

    let block = &rest[..end];
    let after = &rest[end + 1 + FENCE.len()..];
    (Some(block), after.strip_prefix('\n').unwrap_or(after))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalars_render_in_a_fenced_block() {
        let mut fm = Frontmatter::new();
        fm.set("kind", "file").set("path", "src/a.ts");

        let rendered = fm.render();

        assert!(rendered.starts_with("---\n"));
        assert!(rendered.contains("kind: file"));
        assert!(rendered.contains("path: src/a.ts"));
        assert!(rendered.trim_end().ends_with("---"));
    }

    #[test]
    fn lists_render_as_yaml_sequences() {
        let mut fm = Frontmatter::new();
        fm.set_list("tags", vec!["cache".to_string(), "storage".to_string()]);

        let rendered = fm.render();

        assert!(rendered.contains("tags:\n"));
        assert!(rendered.contains("  - cache\n"));
    }

    #[test]
    fn an_empty_list_renders_as_empty_brackets() {
        let mut fm = Frontmatter::new();
        fm.set_list("tags", Vec::new());

        assert!(fm.render().contains("tags: []"));
    }

    #[test]
    fn nothing_to_say_renders_nothing() {
        assert_eq!(Frontmatter::new().render(), "");
    }

    #[test]
    fn output_is_ordered_regardless_of_insertion_order() {
        let mut forward = Frontmatter::new();
        forward.set("a", "1").set("z", "2").set("m", "3");

        let mut backward = Frontmatter::new();
        backward.set("z", "2").set("m", "3").set("a", "1");

        assert_eq!(forward.render(), backward.render());
    }

    /// Rust paths contain `::`, and a bare `a: b: c` is invalid YAML.
    #[test]
    fn values_containing_colons_are_quoted() {
        let mut fm = Frontmatter::new();
        fm.set("name", "std::collections: BTreeMap");

        assert!(fm.render().contains(r#""std::collections: BTreeMap""#));
    }

    #[test]
    fn values_that_look_like_other_types_are_quoted() {
        let mut fm = Frontmatter::new();
        fm.set("a", "true").set("b", "42").set("c", "null");

        let rendered = fm.render();

        assert!(rendered.contains(r#"a: "true""#));
        assert!(rendered.contains(r#"b: "42""#));
        assert!(rendered.contains(r#"c: "null""#));
    }

    #[test]
    fn values_starting_with_yaml_sigils_are_quoted() {
        let mut fm = Frontmatter::new();
        fm.set("pointer", "*mut Cache").set("anchor", "#main");

        let rendered = fm.render();

        assert!(rendered.contains(r#""*mut Cache""#));
        assert!(rendered.contains(r##""#main""##));
    }

    #[test]
    fn quotes_inside_values_are_escaped() {
        let mut fm = Frontmatter::new();
        fm.set("name", r#"say "hi""#);

        assert!(fm.render().contains(r#"\"hi\""#));
    }

    #[test]
    fn a_document_splits_into_frontmatter_and_body() {
        let doc = "---\nkind: file\n---\n# Title\n\nbody";

        let (fm, body) = split(doc);

        assert_eq!(fm, Some("kind: file"));
        assert_eq!(body, "# Title\n\nbody");
    }

    #[test]
    fn a_document_without_frontmatter_is_all_body() {
        let doc = "# Title\n\nbody";

        let (fm, body) = split(doc);

        assert_eq!(fm, None);
        assert_eq!(body, doc);
    }

    /// An unclosed fence would otherwise swallow the entire document.
    #[test]
    fn an_unterminated_fence_is_not_frontmatter() {
        let doc = "---\nkind: file\n\nstill body";

        let (fm, body) = split(doc);

        assert_eq!(fm, None);
        assert_eq!(body, doc);
    }

    #[test]
    fn rendered_frontmatter_round_trips_through_split() {
        let mut fm = Frontmatter::new();
        fm.set("kind", "symbol").set("name", "Cache");

        let doc = format!("{}# Body\n", fm.render());
        let (block, body) = split(&doc);

        assert!(block.expect("frontmatter").contains("name: Cache"));
        assert_eq!(body, "# Body\n");
    }
}
