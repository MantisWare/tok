//! Go extraction.
//!
//! Go's distinguishing feature for graph building is that a method's receiver
//! type is declared in the method itself (`func (c *Cache) Get()`), which makes
//! receiver-typed resolution far more reliable here than in dynamic languages.
//! Interface satisfaction, by contrast, is structural and never written down,
//! so Go produces no `IMPLEMENTS` edges.

use tree_sitter::Node;

use super::{ImportBinding, LanguageExtractor, Receiver};
use crate::graph::extract::source;
use crate::graph::types::{EdgeKind, NodeKind};

pub struct GoExtractor;

impl LanguageExtractor for GoExtractor {
    fn declaration_kind(&self, node: Node, src: &str) -> Option<NodeKind> {
        Some(match node.kind() {
            "function_declaration" => NodeKind::Function,
            "method_declaration" => NodeKind::Method,
            "type_spec" => match underlying_type_kind(node, src) {
                Some(kind) => kind,
                None => NodeKind::Type,
            },
            "const_spec" => NodeKind::Constant,
            _ => return None,
        })
    }

    fn name_of(&self, node: Node, src: &str) -> Option<String> {
        let name = node.child_by_field_name("name")?;
        Some(source::slice(src, name.start_byte(), name.end_byte()).to_string())
    }

    fn is_exported(&self, node: Node, src: &str) -> bool {
        // Go's rule is exact and syntactic: an initial capital is exported.
        self.name_of(node, src)
            .and_then(|n| n.chars().next())
            .is_some_and(|c| c.is_uppercase())
    }

    fn call_target(&self, node: Node, src: &str) -> Option<(Option<Receiver>, String)> {
        if node.kind() != "call_expression" {
            return None;
        }
        let func = node.child_by_field_name("function")?;

        match func.kind() {
            "identifier" => Some((
                None,
                source::slice(src, func.start_byte(), func.end_byte()).to_string(),
            )),
            "selector_expression" => {
                let field = func.child_by_field_name("field")?;
                let name = source::slice(src, field.start_byte(), field.end_byte()).to_string();
                let receiver = func
                    .child_by_field_name("operand")
                    .map(|o| receiver_from(src, o));
                Some((receiver, name))
            }
            _ => None,
        }
    }

    fn imports(&self, node: Node, src: &str) -> Vec<ImportBinding> {
        if node.kind() != "import_spec" {
            return Vec::new();
        }

        let Some(path_node) = node.child_by_field_name("path") else {
            return Vec::new();
        };
        let module = source::slice(src, path_node.start_byte(), path_node.end_byte())
            .trim_matches('"')
            .to_string();

        // The package name is the last path segment unless aliased.
        let leaf = module.rsplit('/').next().unwrap_or(&module).to_string();
        let local = node
            .child_by_field_name("name")
            .map(|n| source::slice(src, n.start_byte(), n.end_byte()).to_string())
            .unwrap_or_else(|| leaf.clone());

        vec![ImportBinding {
            local,
            imported: leaf,
            module,
        }]
    }

    fn inheritance(&self, node: Node, src: &str) -> Vec<(EdgeKind, String)> {
        // Go has no `implements` keyword — satisfaction is structural and
        // invisible to a parser. Embedded types are the closest analogue, and
        // they genuinely do promote methods, so they are Extends edges.
        if node.kind() != "type_spec" {
            return Vec::new();
        }
        let Some(ty) = node.child_by_field_name("type") else {
            return Vec::new();
        };
        if ty.kind() != "struct_type" {
            return Vec::new();
        }

        let mut out = Vec::new();
        // `struct_type` holds a `field_declaration_list` as a plain named child
        // rather than under a `body` field.
        let mut struct_cursor = ty.walk();
        let Some(fields) = ty
            .named_children(&mut struct_cursor)
            .find(|c| c.kind() == "field_declaration_list")
        else {
            return out;
        };

        let mut cursor = fields.walk();
        for field in fields.named_children(&mut cursor) {
            if field.kind() != "field_declaration" {
                continue;
            }
            // An embedded field has a type but no name.
            if field.child_by_field_name("name").is_some() {
                continue;
            }
            if let Some(field_type) = field.child_by_field_name("type") {
                let name = base_type_name(source::slice(
                    src,
                    field_type.start_byte(),
                    field_type.end_byte(),
                ));
                if !name.is_empty() {
                    out.push((EdgeKind::Extends, name));
                }
            }
        }
        out
    }

    fn owner_type_name(&self, node: Node, src: &str) -> Option<String> {
        // `func (c *Cache) Get()` — the receiver names the owning type, which
        // may well be declared in a different file of the same package.
        if node.kind() != "method_declaration" {
            return None;
        }
        let receiver = node.child_by_field_name("receiver")?;

        let mut cursor = receiver.walk();
        for param in receiver.named_children(&mut cursor) {
            if param.kind() != "parameter_declaration" {
                continue;
            }
            if let Some(ty) = param.child_by_field_name("type") {
                let name = base_type_name(source::slice(src, ty.start_byte(), ty.end_byte()));
                if !name.is_empty() {
                    return Some(name);
                }
            }
        }
        None
    }

    fn type_bindings(&self, node: Node, src: &str) -> Vec<(String, String)> {
        match node.kind() {
            // `func (c *Cache) Get()` — the strongest binding Go offers.
            "method_declaration" => {
                let Some(receiver) = node.child_by_field_name("receiver") else {
                    return Vec::new();
                };
                let mut cursor = receiver.walk();
                for param in receiver.named_children(&mut cursor) {
                    if param.kind() != "parameter_declaration" {
                        continue;
                    }
                    let (Some(name), Some(ty)) = (
                        param.child_by_field_name("name"),
                        param.child_by_field_name("type"),
                    ) else {
                        continue;
                    };
                    let var = source::slice(src, name.start_byte(), name.end_byte()).to_string();
                    let type_name =
                        base_type_name(source::slice(src, ty.start_byte(), ty.end_byte()));
                    if !var.is_empty() && !type_name.is_empty() {
                        return vec![(var, type_name)];
                    }
                }
                Vec::new()
            }
            // `var c *Cache` / `c := &Cache{}`
            "var_spec" => {
                let (Some(name), Some(ty)) = (
                    node.child_by_field_name("name"),
                    node.child_by_field_name("type"),
                ) else {
                    return Vec::new();
                };
                let var = source::slice(src, name.start_byte(), name.end_byte()).to_string();
                let type_name = base_type_name(source::slice(src, ty.start_byte(), ty.end_byte()));
                if var.is_empty() || type_name.is_empty() {
                    return Vec::new();
                }
                vec![(var, type_name)]
            }
            "short_var_declaration" => {
                let (Some(left), Some(right)) = (
                    node.child_by_field_name("left"),
                    node.child_by_field_name("right"),
                ) else {
                    return Vec::new();
                };
                let var = source::slice(src, left.start_byte(), left.end_byte()).to_string();
                let init = source::slice(src, right.start_byte(), right.end_byte());
                let type_name = composite_literal_type(init);
                if var.is_empty() || type_name.is_empty() || var.contains(',') {
                    return Vec::new();
                }
                vec![(var, type_name)]
            }
            _ => Vec::new(),
        }
    }
}

/// Classify a `type X ...` declaration by what follows the name.
fn underlying_type_kind(node: Node, _src: &str) -> Option<NodeKind> {
    let ty = node.child_by_field_name("type")?;
    Some(match ty.kind() {
        "struct_type" => NodeKind::Struct,
        "interface_type" => NodeKind::Interface,
        _ => NodeKind::Type,
    })
}

fn receiver_from(src: &str, operand: Node) -> Receiver {
    let text = source::slice(src, operand.start_byte(), operand.end_byte());
    Receiver::Untyped(text.to_string())
}

/// Pull `Cache` out of `&Cache{...}` or `Cache{...}`.
fn composite_literal_type(init: &str) -> String {
    let trimmed = init.trim().trim_start_matches('&');
    let head = trimmed.split(['{', '(']).next().unwrap_or("").trim();
    if head.is_empty() || head.contains(' ') {
        return String::new();
    }
    base_type_name(head)
}

/// Strip pointers, slices, and package qualifiers.
fn base_type_name(raw: &str) -> String {
    let trimmed = raw.trim().trim_start_matches(['*', '&']);
    let trimmed = trimmed.trim_start_matches("[]").trim();
    // `pkg.Type` refers to Type in another package; the bare name is what
    // resolution matches on.
    let head = trimmed.split(['[', '{', ' ']).next().unwrap_or("").trim();
    head.rsplit('.').next().unwrap_or(head).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::extract::{extract_file, FileExtraction};
    use crate::graph::Language;

    fn extract(src: &str) -> FileExtraction {
        extract_file("cache.go", src, Language::Go)
            .expect("extraction succeeds")
            .expect("go grammar available")
    }

    #[test]
    fn structs_interfaces_and_funcs() {
        let out = extract(
            "package m\n\
             type Entry struct { Key string }\n\
             type Storable interface { Label() string }\n\
             func Build() {}\n",
        );
        let kinds: Vec<_> = out
            .nodes
            .iter()
            .map(|n| (n.name.as_str(), n.kind))
            .collect();
        assert!(kinds.contains(&("Entry", NodeKind::Struct)));
        assert!(kinds.contains(&("Storable", NodeKind::Interface)));
        assert!(kinds.contains(&("Build", NodeKind::Function)));
    }

    #[test]
    fn methods_are_methods() {
        let out = extract("package m\ntype C struct{}\nfunc (c *C) Get() {}\n");
        let get = out.nodes.iter().find(|n| n.name == "Get").unwrap();
        assert_eq!(get.kind, NodeKind::Method);
    }

    #[test]
    fn capitalization_decides_export() {
        let out = extract("package m\nfunc Public() {}\nfunc private() {}\n");
        assert!(
            out.nodes
                .iter()
                .find(|n| n.name == "Public")
                .unwrap()
                .exported
        );
        assert!(
            !out.nodes
                .iter()
                .find(|n| n.name == "private")
                .unwrap()
                .exported
        );
    }

    #[test]
    fn method_receiver_binds_its_type() {
        let out = extract("package m\ntype C struct{}\nfunc (c *C) Get() { c.other() }\n");
        assert!(out.bindings.contains(&("c".to_string(), "C".to_string())));
    }

    #[test]
    fn short_var_declaration_binds_composite_literals() {
        let out = extract("package m\nfunc f() { c := &Cache{}\n c.Get() }\n");
        assert!(out
            .bindings
            .contains(&("c".to_string(), "Cache".to_string())));
    }

    #[test]
    fn embedded_struct_fields_are_extends_edges() {
        let out = extract("package m\ntype Base struct{}\ntype C struct { Base\n Name string }\n");
        assert!(out
            .refs
            .iter()
            .any(|r| r.kind == EdgeKind::Extends && r.name == "Base"));
    }

    #[test]
    fn named_fields_are_not_inheritance() {
        let out = extract("package m\ntype C struct { Name string }\n");
        assert!(!out.refs.iter().any(|r| r.kind == EdgeKind::Extends));
    }

    #[test]
    fn imports_use_the_last_path_segment() {
        let out = extract("package m\nimport \"net/http\"\n");
        let imp = out.imports.first().expect("one import");
        assert_eq!(imp.local, "http");
        assert_eq!(imp.module, "net/http");
    }

    #[test]
    fn aliased_imports_keep_the_alias() {
        let out = extract("package m\nimport f \"fmt\"\n");
        let imp = out.imports.first().expect("one import");
        assert_eq!(imp.local, "f");
        assert_eq!(imp.module, "fmt");
    }

    #[test]
    fn selector_calls_record_their_operand() {
        let out = extract("package m\nfunc f(c *Cache) { c.Get() }\n");
        let call = out.refs.iter().find(|r| r.name == "Get").unwrap();
        assert_eq!(call.receiver, Some(Receiver::Untyped("c".to_string())));
    }

    #[test]
    fn type_names_lose_pointers_slices_and_packages() {
        assert_eq!(base_type_name("*Cache"), "Cache");
        assert_eq!(base_type_name("[]Entry"), "Entry");
        assert_eq!(base_type_name("http.Client"), "Client");
    }

    #[test]
    fn composite_literal_types_are_recognized() {
        assert_eq!(composite_literal_type("&Cache{}"), "Cache");
        assert_eq!(composite_literal_type("Cache{a: 1}"), "Cache");
        assert_eq!(composite_literal_type("42"), "42");
    }
}
