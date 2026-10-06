//! Rust extraction.
//!
//! Named `rust_lang` because `rust` is not a usable module name here.
//!
//! Rust needs handling that graft's TypeScript-first design does not cover:
//! inherent and trait `impl` blocks are separate AST nodes from the type they
//! target, so a method's owning type comes from the enclosing `impl`, not from
//! a lexical parent. Getting this wrong is what makes `trait Store::get` and
//! `impl Store for MemoryStore::get` collide in the regex indexer.

use tree_sitter::Node;

use super::{ImportBinding, LanguageExtractor, Receiver};
use crate::graph::extract::source;
use crate::graph::types::{EdgeKind, NodeKind};

pub struct RustExtractor;

impl LanguageExtractor for RustExtractor {
    fn declaration_kind(&self, node: Node, _src: &str) -> Option<NodeKind> {
        Some(match node.kind() {
            "function_item" => {
                // A function inside an impl block is a method; the same AST node
                // type serves both.
                if in_impl_block(node) {
                    NodeKind::Method
                } else {
                    NodeKind::Function
                }
            }
            "function_signature_item" => NodeKind::Method,
            "struct_item" => NodeKind::Struct,
            "enum_item" => NodeKind::Enum,
            "trait_item" => NodeKind::Trait,
            "type_item" => NodeKind::Type,
            "const_item" | "static_item" => NodeKind::Constant,
            "mod_item" => NodeKind::Module,
            "use_declaration" => NodeKind::Import,
            _ => return None,
        })
    }

    fn name_of(&self, node: Node, src: &str) -> Option<String> {
        if node.kind() == "use_declaration" {
            // The whole `use` line is the name, matching how the regex indexer
            // renders imports today — including dropping the trailing
            // semicolon, which reads as noise in symbol listings.
            let text =
                source::collapse_whitespace(source::slice(src, node.start_byte(), node.end_byte()));
            return Some(text.trim_end_matches(';').trim_end().to_string());
        }

        let name = node.child_by_field_name("name")?;
        Some(source::slice(src, name.start_byte(), name.end_byte()).to_string())
    }

    fn is_exported(&self, node: Node, _src: &str) -> bool {
        let mut cursor = node.walk();
        if node
            .children(&mut cursor)
            .any(|c| c.kind() == "visibility_modifier")
        {
            return true;
        }
        // A trait's required methods are part of its public surface even though
        // they carry no `pub` of their own.
        node.kind() == "function_signature_item"
    }

    fn call_target(&self, node: Node, src: &str) -> Option<(Option<Receiver>, String)> {
        if node.kind() != "call_expression" {
            return None;
        }
        let func = node.child_by_field_name("function")?;

        match func.kind() {
            // `foo()`
            "identifier" => Some((
                None,
                source::slice(src, func.start_byte(), func.end_byte()).to_string(),
            )),
            // `self.foo()`, `x.foo()`
            "field_expression" => {
                let field = func.child_by_field_name("field")?;
                let name = source::slice(src, field.start_byte(), field.end_byte()).to_string();
                let receiver = func
                    .child_by_field_name("value")
                    .map(|v| receiver_from(src, v));
                Some((receiver, name))
            }
            // `Type::foo()`, `module::foo()`
            "scoped_identifier" => {
                let name_node = func.child_by_field_name("name")?;
                let name = source::slice(src, name_node.start_byte(), name_node.end_byte());
                let qualifier = func
                    .child_by_field_name("path")
                    .map(|p| source::slice(src, p.start_byte(), p.end_byte()).to_string());
                // `Self::f()` targets the enclosing type.
                let receiver = match qualifier.as_deref() {
                    Some("Self") => Some(Receiver::SelfType),
                    Some(q) => Some(Receiver::Typed(q.to_string())),
                    None => None,
                };
                Some((receiver, name.to_string()))
            }
            _ => None,
        }
    }

    fn imports(&self, node: Node, src: &str) -> Vec<ImportBinding> {
        if node.kind() != "use_declaration" {
            return Vec::new();
        }
        let text = source::slice(src, node.start_byte(), node.end_byte());
        parse_use_declaration(text)
    }

    fn inheritance(&self, _node: Node, _src: &str) -> Vec<(EdgeKind, String)> {
        // Rust declares inheritance in `impl` blocks, which are not
        // declarations themselves. See `named_relations`.
        Vec::new()
    }

    fn named_relations(&self, node: Node, src: &str) -> Vec<super::NamedRelation> {
        // `impl Trait for Type` relates two names that are usually declared
        // elsewhere, so the edge cannot be attached to the impl block itself.
        if node.kind() != "impl_item" {
            return Vec::new();
        }

        let (Some(trait_node), Some(type_node)) = (
            node.child_by_field_name("trait"),
            node.child_by_field_name("type"),
        ) else {
            // An inherent `impl Type { .. }` has no trait, so nothing to relate.
            return Vec::new();
        };

        let trait_name = base_type_name(source::slice(
            src,
            trait_node.start_byte(),
            trait_node.end_byte(),
        ));
        let type_name = base_type_name(source::slice(
            src,
            type_node.start_byte(),
            type_node.end_byte(),
        ));

        if trait_name.is_empty() || type_name.is_empty() {
            return Vec::new();
        }

        vec![super::NamedRelation {
            from_name: type_name,
            to_name: trait_name,
            kind: EdgeKind::Implements,
        }]
    }

    fn type_bindings(&self, node: Node, src: &str) -> Vec<(String, String)> {
        // `let c: Cache = ...` and `let c = Cache::new()` both tell us c's type.
        if node.kind() != "let_declaration" {
            return Vec::new();
        }

        let Some(pattern) = node.child_by_field_name("pattern") else {
            return Vec::new();
        };
        let var = source::slice(src, pattern.start_byte(), pattern.end_byte()).to_string();
        if var.is_empty() {
            return Vec::new();
        }

        if let Some(ty) = node.child_by_field_name("type") {
            let name = base_type_name(source::slice(src, ty.start_byte(), ty.end_byte()));
            if !name.is_empty() {
                return vec![(var, name)];
            }
        }

        // Fall back to the constructor: `Cache::new()` implies `Cache`.
        if let Some(value) = node.child_by_field_name("value") {
            if value.kind() == "call_expression" {
                if let Some(func) = value.child_by_field_name("function") {
                    if func.kind() == "scoped_identifier" {
                        if let Some(path) = func.child_by_field_name("path") {
                            let ty = source::slice(src, path.start_byte(), path.end_byte());
                            if !ty.is_empty() {
                                return vec![(var, ty.to_string())];
                            }
                        }
                    }
                }
            }
        }

        Vec::new()
    }
}

/// Whether this node sits inside an `impl` block.
///
/// Walks ancestors rather than checking the immediate parent, because the AST
/// puts a `declaration_list` between an `impl_item` and its functions.
fn in_impl_block(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(n) = current {
        match n.kind() {
            "impl_item" | "trait_item" => return true,
            // Stop at the next function boundary: a nested fn inside a method
            // body is a plain function, not a method.
            "function_item" => return false,
            _ => current = n.parent(),
        }
    }
    false
}

/// Strip generics and references from a type to get its bare name.
///
/// `&mut Vec<Cache>` becomes `Vec`, because edges point at the named type.
fn base_type_name(raw: &str) -> String {
    let trimmed = raw.trim().trim_start_matches(['&', '*']).trim();
    let trimmed = trimmed.strip_prefix("mut ").unwrap_or(trimmed).trim();
    let trimmed = trimmed.strip_prefix("dyn ").unwrap_or(trimmed).trim();

    let head = trimmed
        .split(['<', '(', ' ', ':'])
        .find(|s| !s.is_empty())
        .unwrap_or("");
    head.to_string()
}

/// Build a receiver from the value side of a field expression.
fn receiver_from(src: &str, value: Node) -> Receiver {
    let text = source::slice(src, value.start_byte(), value.end_byte());
    if text == "self" || text == "Self" {
        Receiver::SelfType
    } else {
        Receiver::Untyped(text.to_string())
    }
}

/// Parse `use a::b::{c, d as e};` into its bindings.
fn parse_use_declaration(text: &str) -> Vec<ImportBinding> {
    let body = text
        .trim()
        .trim_start_matches("pub ")
        .trim_start_matches("use ")
        .trim_end_matches(';')
        .trim();

    let mut out = Vec::new();

    if let Some((prefix, group)) = body.split_once('{') {
        let module = prefix.trim().trim_end_matches("::").to_string();
        for item in group.trim_end_matches('}').split(',') {
            if let Some(binding) = binding_from_item(item.trim(), &module) {
                out.push(binding);
            }
        }
        return out;
    }

    if let Some(binding) = binding_from_path(body) {
        out.push(binding);
    }
    out
}

fn binding_from_item(item: &str, module: &str) -> Option<ImportBinding> {
    if item.is_empty() || item == "self" {
        return None;
    }
    let (imported, local) = split_alias(item);
    Some(ImportBinding {
        local,
        imported,
        module: module.to_string(),
    })
}

fn binding_from_path(path: &str) -> Option<ImportBinding> {
    if path.is_empty() || path.ends_with('*') {
        return None;
    }
    let (full, alias) = split_alias(path);
    let imported = full.rsplit("::").next()?.to_string();
    let module = full
        .rsplit_once("::")
        .map(|(m, _)| m.to_string())
        .unwrap_or_default();

    let local = if alias == full {
        imported.clone()
    } else {
        alias
    };

    Some(ImportBinding {
        local,
        imported,
        module,
    })
}

/// Split `x as y` into (x, y); without an alias both halves are `x`.
fn split_alias(item: &str) -> (String, String) {
    match item.split_once(" as ") {
        Some((orig, alias)) => (orig.trim().to_string(), alias.trim().to_string()),
        None => (item.trim().to_string(), item.trim().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::extract::extract_file;
    use crate::graph::Language;

    fn extract(src: &str) -> crate::graph::extract::FileExtraction {
        extract_file("lib.rs", src, Language::Rust)
            .expect("extraction succeeds")
            .expect("rust grammar available")
    }

    #[test]
    fn finds_functions_structs_traits_and_enums() {
        let out = extract(
            "pub struct Entry {}\npub enum Status { A }\npub trait Store {}\npub fn f() {}",
        );
        let kinds: Vec<_> = out
            .nodes
            .iter()
            .filter(|n| n.kind != NodeKind::File)
            .map(|n| (n.name.as_str(), n.kind))
            .collect();

        assert!(kinds.contains(&("Entry", NodeKind::Struct)));
        assert!(kinds.contains(&("Status", NodeKind::Enum)));
        assert!(kinds.contains(&("Store", NodeKind::Trait)));
        assert!(kinds.contains(&("f", NodeKind::Function)));
    }

    /// The collision the regex indexer cannot represent.
    #[test]
    fn trait_method_and_impl_method_get_distinct_ids() {
        let out = extract(
            "pub trait Store { fn get(&self) -> u32; }\n\
             pub struct M;\n\
             impl Store for M { fn get(&self) -> u32 { 0 } }",
        );

        let gets: Vec<_> = out.nodes.iter().filter(|n| n.name == "get").collect();
        assert_eq!(gets.len(), 2, "both declarations are present");
        assert_ne!(gets[0].id, gets[1].id, "ids must differ");
        assert!(
            gets.iter().any(|n| n.id.contains('~')),
            "one gets an ordinal"
        );
    }

    #[test]
    fn impl_methods_are_methods_not_functions() {
        let out = extract("struct M;\nimpl M { fn go(&self) {} }");
        let go = out.nodes.iter().find(|n| n.name == "go").unwrap();
        assert_eq!(go.kind, NodeKind::Method);
    }

    #[test]
    fn a_nested_fn_inside_a_method_is_a_function() {
        let out = extract("struct M;\nimpl M { fn go(&self) { fn helper() {} } }");
        let helper = out.nodes.iter().find(|n| n.name == "helper").unwrap();
        assert_eq!(helper.kind, NodeKind::Function);
    }

    #[test]
    fn records_plain_calls() {
        let out = extract("fn a() {}\nfn b() { a(); }");
        assert!(out
            .refs
            .iter()
            .any(|r| r.name == "a" && r.kind == EdgeKind::Calls));
    }

    #[test]
    fn self_calls_are_marked_as_self_receivers() {
        let out = extract("struct M;\nimpl M { fn a(&self) {} fn b(&self) { self.a(); } }");
        let call = out
            .refs
            .iter()
            .find(|r| r.name == "a" && r.kind == EdgeKind::Calls)
            .expect("self.a() recorded");
        assert_eq!(call.receiver, Some(Receiver::SelfType));
    }

    #[test]
    fn impl_for_relates_the_type_to_the_trait() {
        let out = extract("struct M;\ntrait T {}\nimpl T for M {}");
        let relation = out
            .named_relations
            .iter()
            .find(|r| r.kind == EdgeKind::Implements)
            .expect("implements relation recorded");
        assert_eq!(relation.from_name, "M", "edge starts at the type");
        assert_eq!(relation.to_name, "T", "edge points at the trait");
    }

    #[test]
    fn inherent_impl_produces_no_implements_edge() {
        let out = extract("struct M;\nimpl M { fn f(&self) {} }");
        assert!(out.named_relations.is_empty());
        assert!(!out.refs.iter().any(|r| r.kind == EdgeKind::Implements));
    }

    #[test]
    fn generic_impls_reduce_to_base_names() {
        let out = extract("struct M<T>;\ntrait Store<T> {}\nimpl<T> Store<T> for M<T> {}");
        let relation = out.named_relations.first().expect("relation recorded");
        assert_eq!(relation.from_name, "M");
        assert_eq!(relation.to_name, "Store");
    }

    #[test]
    fn visibility_marks_exported() {
        let out = extract("pub fn shown() {}\nfn hidden() {}");
        let shown = out.nodes.iter().find(|n| n.name == "shown").unwrap();
        let hidden = out.nodes.iter().find(|n| n.name == "hidden").unwrap();
        assert!(shown.exported);
        assert!(!hidden.exported);
    }

    #[test]
    fn spans_cover_the_whole_declaration() {
        let out = extract("pub fn f() {\n    let x = 1;\n}\n");
        let f = out.nodes.iter().find(|n| n.name == "f").unwrap();
        assert_eq!(f.span.start, 1);
        assert_eq!(f.span.end, 3, "span reaches the closing brace");
    }

    #[test]
    fn captures_doc_comments() {
        let out = extract("/// Builds a store.\npub fn build() {}");
        let f = out.nodes.iter().find(|n| n.name == "build").unwrap();
        assert_eq!(f.doc.as_deref(), Some("Builds a store."));
    }

    #[test]
    fn parses_grouped_use_declarations() {
        let bindings = parse_use_declaration("use std::collections::{HashMap, BTreeMap as BT};");
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].imported, "HashMap");
        assert_eq!(bindings[0].module, "std::collections");
        assert_eq!(bindings[1].imported, "BTreeMap");
        assert_eq!(bindings[1].local, "BT");
    }

    #[test]
    fn parses_simple_use_declarations() {
        let bindings = parse_use_declaration("use anyhow::Result;");
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].imported, "Result");
        assert_eq!(bindings[0].module, "anyhow");
    }

    #[test]
    fn glob_imports_bind_nothing() {
        assert!(parse_use_declaration("use prelude::*;").is_empty());
    }

    /// Import symbols are listed by their whole `use` line, and a dangling
    /// semicolon there is pure noise in `tok mem detect` output.
    #[test]
    fn import_symbol_names_drop_the_trailing_semicolon() {
        let extraction = extract("use std::collections::HashMap;");
        let import = extraction
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Import)
            .expect("import node");

        assert_eq!(import.name, "use std::collections::HashMap");
    }

    #[test]
    fn strips_generics_and_references_from_type_names() {
        assert_eq!(base_type_name("&mut Vec<Cache>"), "Vec");
        assert_eq!(base_type_name("Cache"), "Cache");
        assert_eq!(base_type_name("dyn Store"), "Store");
        assert_eq!(base_type_name("&'a str"), "'a");
    }

    #[test]
    fn let_bindings_record_types() {
        let out = extract("fn f() { let c: Cache = make(); c.get(); }");
        let call = out.refs.iter().find(|r| r.name == "get").unwrap();
        assert_eq!(call.receiver, Some(Receiver::Untyped("c".to_string())));
    }
}
