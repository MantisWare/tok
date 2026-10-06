//! Python extraction.
//!
//! Python has no export keyword, so visibility follows the underscore
//! convention. It also has no type declarations distinct from classes, so
//! `class` covers what other languages split across class, struct, and
//! interface.

use tree_sitter::Node;

use super::{ImportBinding, LanguageExtractor, Receiver};
use crate::graph::extract::source;
use crate::graph::types::{EdgeKind, NodeKind};

pub struct PythonExtractor;

impl LanguageExtractor for PythonExtractor {
    fn declaration_kind(&self, node: Node, _src: &str) -> Option<NodeKind> {
        Some(match node.kind() {
            "function_definition" => {
                if in_class_body(node) {
                    NodeKind::Method
                } else {
                    NodeKind::Function
                }
            }
            "class_definition" => NodeKind::Class,
            _ => return None,
        })
    }

    fn name_of(&self, node: Node, src: &str) -> Option<String> {
        let name = node.child_by_field_name("name")?;
        Some(source::slice(src, name.start_byte(), name.end_byte()).to_string())
    }

    fn is_exported(&self, node: Node, src: &str) -> bool {
        // Python's only visibility signal is the leading-underscore convention.
        // `__init__` and friends are dunder methods, which are public API.
        let Some(name) = self.name_of(node, src) else {
            return false;
        };
        if name.starts_with("__") && name.ends_with("__") {
            return true;
        }
        !name.starts_with('_')
    }

    fn call_target(&self, node: Node, src: &str) -> Option<(Option<Receiver>, String)> {
        if node.kind() != "call" {
            return None;
        }
        let func = node.child_by_field_name("function")?;

        match func.kind() {
            "identifier" => Some((
                None,
                source::slice(src, func.start_byte(), func.end_byte()).to_string(),
            )),
            "attribute" => {
                let attr = func.child_by_field_name("attribute")?;
                let name = source::slice(src, attr.start_byte(), attr.end_byte()).to_string();
                let receiver = func
                    .child_by_field_name("object")
                    .map(|o| receiver_from(src, o));
                Some((receiver, name))
            }
            _ => None,
        }
    }

    fn imports(&self, node: Node, src: &str) -> Vec<ImportBinding> {
        match node.kind() {
            // `import a.b as c`
            "import_statement" => {
                let mut out = Vec::new();
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if let Some(binding) = simple_import(child, src) {
                        out.push(binding);
                    }
                }
                out
            }
            // `from m import a, b as c`
            "import_from_statement" => {
                let module = node
                    .child_by_field_name("module_name")
                    .map(|m| source::slice(src, m.start_byte(), m.end_byte()).to_string())
                    .unwrap_or_default();

                let mut out = Vec::new();
                let mut cursor = node.walk();
                for child in node.children_by_field_name("name", &mut cursor) {
                    if let Some((imported, local)) = aliased_name(child, src) {
                        out.push(ImportBinding {
                            local,
                            imported,
                            module: module.clone(),
                        });
                    }
                }
                out
            }
            _ => Vec::new(),
        }
    }

    fn inheritance(&self, node: Node, src: &str) -> Vec<(EdgeKind, String)> {
        if node.kind() != "class_definition" {
            return Vec::new();
        }
        let Some(supers) = node.child_by_field_name("superclasses") else {
            return Vec::new();
        };

        let mut out = Vec::new();
        let mut cursor = supers.walk();
        for child in supers.named_children(&mut cursor) {
            // Skip keyword arguments like `metaclass=ABCMeta`.
            if child.kind() == "keyword_argument" {
                continue;
            }
            let name = base_type_name(source::slice(src, child.start_byte(), child.end_byte()));
            if !name.is_empty() {
                // Python makes no syntactic distinction between inheriting an
                // implementation and satisfying a protocol, so every base is an
                // Extends edge.
                out.push((EdgeKind::Extends, name));
            }
        }
        out
    }

    fn type_bindings(&self, node: Node, src: &str) -> Vec<(String, String)> {
        if node.kind() != "assignment" {
            return Vec::new();
        }

        let Some(left) = node.child_by_field_name("left") else {
            return Vec::new();
        };
        let var = source::slice(src, left.start_byte(), left.end_byte()).to_string();
        if var.is_empty() || var.contains('.') {
            return Vec::new();
        }

        // `c: Cache = ...`
        if let Some(ty) = node.child_by_field_name("type") {
            let name = base_type_name(source::slice(src, ty.start_byte(), ty.end_byte()));
            if !name.is_empty() {
                return vec![(var, name)];
            }
        }

        // `c = Cache()` — a call to a Capitalized name is conventionally a
        // constructor. This is a heuristic, and resolution drops it when the
        // name turns out not to be a class.
        if let Some(right) = node.child_by_field_name("right") {
            if right.kind() == "call" {
                if let Some(func) = right.child_by_field_name("function") {
                    if func.kind() == "identifier" {
                        let name = source::slice(src, func.start_byte(), func.end_byte());
                        if name.chars().next().is_some_and(|c| c.is_uppercase()) {
                            return vec![(var, name.to_string())];
                        }
                    }
                }
            }
        }

        Vec::new()
    }
}

/// Whether this function sits directly in a class body.
fn in_class_body(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(n) = current {
        match n.kind() {
            "class_definition" => return true,
            // A closure defined inside a method is a plain function.
            "function_definition" => return false,
            _ => current = n.parent(),
        }
    }
    false
}

fn simple_import(node: Node, src: &str) -> Option<ImportBinding> {
    let (imported, local) = aliased_name(node, src)?;
    // `import a.b.c` binds the top-level package name.
    let module = imported.clone();
    let leaf = imported.rsplit('.').next().unwrap_or(&imported).to_string();
    Some(ImportBinding {
        local: if local == imported {
            leaf.clone()
        } else {
            local
        },
        imported: leaf,
        module,
    })
}

/// Read a name node that may be wrapped in an `aliased_import`.
fn aliased_name(node: Node, src: &str) -> Option<(String, String)> {
    match node.kind() {
        "dotted_name" | "identifier" => {
            let text = source::slice(src, node.start_byte(), node.end_byte()).to_string();
            if text.is_empty() {
                return None;
            }
            Some((text.clone(), text))
        }
        "aliased_import" => {
            let name = node.child_by_field_name("name")?;
            let alias = node.child_by_field_name("alias")?;
            Some((
                source::slice(src, name.start_byte(), name.end_byte()).to_string(),
                source::slice(src, alias.start_byte(), alias.end_byte()).to_string(),
            ))
        }
        _ => None,
    }
}

fn receiver_from(src: &str, object: Node) -> Receiver {
    let text = source::slice(src, object.start_byte(), object.end_byte());
    if text == "self" || text == "cls" {
        Receiver::SelfType
    } else {
        Receiver::Untyped(text.to_string())
    }
}

/// Reduce `List[Cache]` or `Optional[Cache]` to its head.
fn base_type_name(raw: &str) -> String {
    let trimmed = raw.trim().trim_matches(['"', '\'']);
    trimmed
        .split(['[', '(', ' ', ','])
        .find(|s| !s.is_empty())
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::extract::{extract_file, FileExtraction};
    use crate::graph::Language;

    fn extract(src: &str) -> FileExtraction {
        extract_file("cache.py", src, Language::Python)
            .expect("extraction succeeds")
            .expect("python grammar available")
    }

    #[test]
    fn finds_classes_and_functions() {
        let out = extract("class Cache:\n    pass\n\ndef build():\n    pass\n");
        let kinds: Vec<_> = out
            .nodes
            .iter()
            .map(|n| (n.name.as_str(), n.kind))
            .collect();
        assert!(kinds.contains(&("Cache", NodeKind::Class)));
        assert!(kinds.contains(&("build", NodeKind::Function)));
    }

    #[test]
    fn methods_know_their_class() {
        let out = extract("class Cache:\n    def get(self, k):\n        pass\n");
        let cache = out.nodes.iter().find(|n| n.name == "Cache").unwrap();
        let get = out.nodes.iter().find(|n| n.name == "get").unwrap();
        assert_eq!(get.kind, NodeKind::Method);
        assert_eq!(get.parent.as_deref(), Some(cache.id.as_str()));
    }

    #[test]
    fn nested_function_is_not_a_method() {
        let out = extract("class C:\n    def m(self):\n        def inner():\n            pass\n");
        let inner = out.nodes.iter().find(|n| n.name == "inner").unwrap();
        assert_eq!(inner.kind, NodeKind::Function);
    }

    #[test]
    fn underscore_names_are_private_but_dunders_are_not() {
        let out = extract("class C:\n    def _hidden(self): pass\n    def __init__(self): pass\n");
        let hidden = out.nodes.iter().find(|n| n.name == "_hidden").unwrap();
        let init = out.nodes.iter().find(|n| n.name == "__init__").unwrap();
        assert!(!hidden.exported);
        assert!(init.exported, "dunder methods are public API");
    }

    #[test]
    fn base_classes_become_extends_edges() {
        let out = extract("class Cache(BaseCache, Mixin):\n    pass\n");
        let bases: Vec<_> = out
            .refs
            .iter()
            .filter(|r| r.kind == EdgeKind::Extends)
            .map(|r| r.name.as_str())
            .collect();
        assert!(bases.contains(&"BaseCache"));
        assert!(bases.contains(&"Mixin"));
    }

    #[test]
    fn metaclass_keyword_is_not_a_base_class() {
        let out = extract("class C(Base, metaclass=ABCMeta):\n    pass\n");
        let bases: Vec<_> = out
            .refs
            .iter()
            .filter(|r| r.kind == EdgeKind::Extends)
            .map(|r| r.name.as_str())
            .collect();
        assert!(bases.contains(&"Base"));
        assert!(!bases.contains(&"ABCMeta"), "metaclass is not inheritance");
    }

    #[test]
    fn from_imports_with_aliases() {
        let out = extract("from util import normalize, slugify as slug\n");
        let names: Vec<_> = out
            .imports
            .iter()
            .map(|i| (i.imported.as_str(), i.local.as_str(), i.module.as_str()))
            .collect();
        assert!(names.contains(&("normalize", "normalize", "util")));
        assert!(names.contains(&("slugify", "slug", "util")));
    }

    #[test]
    fn plain_imports_bind_the_leaf_name() {
        let out = extract("import os.path\nimport numpy as np\n");
        assert!(out.imports.iter().any(|i| i.local == "path"));
        assert!(out
            .imports
            .iter()
            .any(|i| i.local == "np" && i.module == "numpy"));
    }

    #[test]
    fn self_calls_are_self_receivers() {
        let out = extract("class C:\n    def a(self): pass\n    def b(self): self.a()\n");
        let call = out.refs.iter().find(|r| r.name == "a").unwrap();
        assert_eq!(call.receiver, Some(Receiver::SelfType));
    }

    #[test]
    fn plain_calls_have_no_receiver() {
        let out = extract("def a(): pass\ndef b(): a()\n");
        let call = out.refs.iter().find(|r| r.name == "a").unwrap();
        assert_eq!(call.receiver, None);
    }

    #[test]
    fn constructor_assignment_binds_a_type() {
        let out = extract("def f():\n    c = Cache()\n    c.get('k')\n");
        assert!(out
            .bindings
            .contains(&("c".to_string(), "Cache".to_string())));
    }

    #[test]
    fn lowercase_calls_do_not_bind_types() {
        // `x = helper()` says nothing about x's type.
        let out = extract("def f():\n    x = helper()\n");
        assert!(out.bindings.is_empty());
    }

    #[test]
    fn subscripted_types_reduce_to_their_head() {
        assert_eq!(base_type_name("List[Cache]"), "List");
        assert_eq!(base_type_name("'Cache'"), "Cache");
    }
}
