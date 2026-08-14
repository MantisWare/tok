//! TypeScript, TSX, and JavaScript extraction.
//!
//! One extractor serves all three: the TSX grammar is a superset of JS, and the
//! node types that matter here (`class_declaration`, `function_declaration`,
//! `call_expression`) are identical across them. Type annotations simply do not
//! appear in JS files, so the annotation-reading paths return nothing rather
//! than needing to be disabled.

use tree_sitter::Node;

use super::{ImportBinding, LanguageExtractor, Receiver};
use crate::graph::extract::source;
use crate::graph::types::{EdgeKind, NodeKind};

pub struct TypeScriptExtractor;

impl LanguageExtractor for TypeScriptExtractor {
    fn declaration_kind(&self, node: Node, _src: &str) -> Option<NodeKind> {
        Some(match node.kind() {
            "function_declaration" | "generator_function_declaration" => NodeKind::Function,
            "class_declaration" | "abstract_class_declaration" => NodeKind::Class,
            "interface_declaration" => NodeKind::Interface,
            "type_alias_declaration" => NodeKind::Type,
            "enum_declaration" => NodeKind::Enum,
            "method_definition" | "method_signature" => NodeKind::Method,
            "public_field_definition" | "property_signature" => return None,
            // `const f = () => {}` is a function to a reader, so classify by the
            // initializer rather than by the syntactic form.
            "variable_declarator" => {
                if declarator_is_function(node) {
                    NodeKind::Function
                } else {
                    NodeKind::Constant
                }
            }
            _ => return None,
        })
    }

    fn name_of(&self, node: Node, src: &str) -> Option<String> {
        let name = node.child_by_field_name("name")?;
        let text = source::slice(src, name.start_byte(), name.end_byte());
        if text.is_empty() {
            return None;
        }
        Some(text.to_string())
    }

    fn is_exported(&self, node: Node, _src: &str) -> bool {
        // `export` wraps the declaration in an export_statement, so the marker
        // is on an ancestor rather than the declaration itself.
        let mut current = node.parent();
        while let Some(n) = current {
            match n.kind() {
                "export_statement" => return true,
                // Stop at the first real scope boundary.
                "statement_block" | "class_body" | "program" => return false,
                _ => current = n.parent(),
            }
        }
        false
    }

    fn call_target(&self, node: Node, src: &str) -> Option<(Option<Receiver>, String)> {
        match node.kind() {
            "call_expression" => {
                let func = node.child_by_field_name("function")?;
                match func.kind() {
                    "identifier" => Some((
                        None,
                        source::slice(src, func.start_byte(), func.end_byte()).to_string(),
                    )),
                    "member_expression" => {
                        let prop = func.child_by_field_name("property")?;
                        let name =
                            source::slice(src, prop.start_byte(), prop.end_byte()).to_string();
                        let receiver = func
                            .child_by_field_name("object")
                            .map(|o| receiver_from(src, o));
                        Some((receiver, name))
                    }
                    _ => None,
                }
            }
            // `new Cache()` is a call to the constructor, and more usefully a
            // reference to the class itself.
            "new_expression" => {
                let ctor = node.child_by_field_name("constructor")?;
                if ctor.kind() != "identifier" {
                    return None;
                }
                Some((
                    None,
                    source::slice(src, ctor.start_byte(), ctor.end_byte()).to_string(),
                ))
            }
            _ => None,
        }
    }

    fn imports(&self, node: Node, src: &str) -> Vec<ImportBinding> {
        if node.kind() != "import_statement" {
            return Vec::new();
        }

        let module = node
            .child_by_field_name("source")
            .map(|s| {
                source::slice(src, s.start_byte(), s.end_byte())
                    .trim_matches(['"', '\''])
                    .to_string()
            })
            .unwrap_or_default();

        let mut out = Vec::new();
        let mut cursor = node.walk();

        for child in node.named_children(&mut cursor) {
            if child.kind() != "import_clause" {
                continue;
            }
            collect_import_clause(child, src, &module, &mut out);
        }

        out
    }

    fn inheritance(&self, node: Node, src: &str) -> Vec<(EdgeKind, String)> {
        let mut out = Vec::new();
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            if child.kind() != "class_heritage" && child.kind() != "extends_type_clause" {
                continue;
            }

            let mut inner = child.walk();
            for part in child.children(&mut inner) {
                match part.kind() {
                    "extends_clause" => {
                        collect_heritage(part, src, EdgeKind::Extends, &mut out);
                    }
                    "implements_clause" => {
                        collect_heritage(part, src, EdgeKind::Implements, &mut out);
                    }
                    // An interface's `extends` list is a bare type list.
                    "type_identifier" | "generic_type" => {
                        let name =
                            base_type_name(source::slice(src, part.start_byte(), part.end_byte()));
                        if !name.is_empty() {
                            out.push((EdgeKind::Extends, name));
                        }
                    }
                    _ => {}
                }
            }
        }

        out
    }

    fn type_bindings(&self, node: Node, src: &str) -> Vec<(String, String)> {
        if node.kind() != "variable_declarator" {
            return Vec::new();
        }

        let Some(name_node) = node.child_by_field_name("name") else {
            return Vec::new();
        };
        let var = source::slice(src, name_node.start_byte(), name_node.end_byte()).to_string();
        if var.is_empty() {
            return Vec::new();
        }

        // An explicit annotation is authoritative: `const c: Cache = ...`.
        if let Some(ty) = node.child_by_field_name("type") {
            let text = source::slice(src, ty.start_byte(), ty.end_byte());
            let name = base_type_name(text.trim_start_matches(':').trim());
            if !name.is_empty() {
                return vec![(var, name)];
            }
        }

        // Otherwise infer from `new Cache()`.
        if let Some(value) = node.child_by_field_name("value") {
            if value.kind() == "new_expression" {
                if let Some(ctor) = value.child_by_field_name("constructor") {
                    let name =
                        base_type_name(source::slice(src, ctor.start_byte(), ctor.end_byte()));
                    if !name.is_empty() {
                        return vec![(var, name)];
                    }
                }
            }
        }

        Vec::new()
    }
}

/// Whether a `variable_declarator`'s initializer is a function.
fn declarator_is_function(node: Node) -> bool {
    node.child_by_field_name("value").is_some_and(|v| {
        matches!(
            v.kind(),
            "arrow_function" | "function_expression" | "function" | "generator_function"
        )
    })
}

fn collect_heritage(clause: Node, src: &str, kind: EdgeKind, out: &mut Vec<(EdgeKind, String)>) {
    let mut cursor = clause.walk();
    for child in clause.named_children(&mut cursor) {
        let name = base_type_name(source::slice(src, child.start_byte(), child.end_byte()));
        if !name.is_empty() {
            out.push((kind, name));
        }
    }
}

fn collect_import_clause(clause: Node, src: &str, module: &str, out: &mut Vec<ImportBinding>) {
    let mut cursor = clause.walk();

    for child in clause.named_children(&mut cursor) {
        match child.kind() {
            // `import Default from "m"`
            "identifier" => {
                let local = source::slice(src, child.start_byte(), child.end_byte()).to_string();
                out.push(ImportBinding {
                    local: local.clone(),
                    imported: "default".to_string(),
                    module: module.to_string(),
                });
            }
            // `import { a, b as c } from "m"`
            "named_imports" => {
                let mut inner = child.walk();
                for spec in child.named_children(&mut inner) {
                    if spec.kind() != "import_specifier" {
                        continue;
                    }
                    let Some(name_node) = spec.child_by_field_name("name") else {
                        continue;
                    };
                    let imported = source::slice(src, name_node.start_byte(), name_node.end_byte())
                        .to_string();
                    let local = spec
                        .child_by_field_name("alias")
                        .map(|a| source::slice(src, a.start_byte(), a.end_byte()).to_string())
                        .unwrap_or_else(|| imported.clone());

                    out.push(ImportBinding {
                        local,
                        imported,
                        module: module.to_string(),
                    });
                }
            }
            // `import * as ns from "m"`
            "namespace_import" => {
                let mut inner = child.walk();
                for part in child.named_children(&mut inner) {
                    let local = source::slice(src, part.start_byte(), part.end_byte()).to_string();
                    out.push(ImportBinding {
                        local,
                        imported: "*".to_string(),
                        module: module.to_string(),
                    });
                }
            }
            _ => {}
        }
    }
}

fn receiver_from(src: &str, object: Node) -> Receiver {
    let text = source::slice(src, object.start_byte(), object.end_byte());
    if text == "this" {
        Receiver::SelfType
    } else {
        Receiver::Untyped(text.to_string())
    }
}

/// Reduce `Array<Cache>` or `Cache[]` to `Cache`.
fn base_type_name(raw: &str) -> String {
    let trimmed = raw.trim();
    let head = trimmed
        .split(['<', '[', '(', ' ', '|', '&'])
        .find(|s| !s.is_empty())
        .unwrap_or("");
    head.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::extract::{extract_file, FileExtraction};
    use crate::graph::Language;

    fn extract(src: &str) -> FileExtraction {
        extract_file("cache.ts", src, Language::TypeScript)
            .expect("extraction succeeds")
            .expect("typescript grammar available")
    }

    #[test]
    fn finds_classes_interfaces_and_functions() {
        let out = extract(
            "export interface Storable { key: string }\n\
             export class Cache {}\n\
             export function build() {}\n\
             export type Key = string;",
        );
        let kinds: Vec<_> = out
            .nodes
            .iter()
            .map(|n| (n.name.as_str(), n.kind))
            .collect();

        assert!(kinds.contains(&("Storable", NodeKind::Interface)));
        assert!(kinds.contains(&("Cache", NodeKind::Class)));
        assert!(kinds.contains(&("build", NodeKind::Function)));
        assert!(kinds.contains(&("Key", NodeKind::Type)));
    }

    #[test]
    fn arrow_consts_are_functions_plain_consts_are_not() {
        let out = extract("const f = () => {};\nconst n = 42;");
        let f = out.nodes.iter().find(|n| n.name == "f").unwrap();
        let n = out.nodes.iter().find(|n| n.name == "n").unwrap();
        assert_eq!(f.kind, NodeKind::Function);
        assert_eq!(n.kind, NodeKind::Constant);
    }

    #[test]
    fn methods_are_methods_and_know_their_class() {
        let out = extract("export class Cache { get(k: string) {} }");
        let cache = out.nodes.iter().find(|n| n.name == "Cache").unwrap();
        let get = out.nodes.iter().find(|n| n.name == "get").unwrap();
        assert_eq!(get.kind, NodeKind::Method);
        assert_eq!(get.parent.as_deref(), Some(cache.id.as_str()));
    }

    #[test]
    fn export_marks_declarations_exported() {
        let out = extract("export function shown() {}\nfunction hidden() {}");
        assert!(
            out.nodes
                .iter()
                .find(|n| n.name == "shown")
                .unwrap()
                .exported
        );
        assert!(
            !out.nodes
                .iter()
                .find(|n| n.name == "hidden")
                .unwrap()
                .exported
        );
    }

    #[test]
    fn extends_and_implements_are_distinguished() {
        let out = extract("class Cache extends Base implements Storable {}");
        assert!(out
            .refs
            .iter()
            .any(|r| r.kind == EdgeKind::Extends && r.name == "Base"));
        assert!(out
            .refs
            .iter()
            .any(|r| r.kind == EdgeKind::Implements && r.name == "Storable"));
    }

    #[test]
    fn named_imports_with_aliases() {
        let out = extract("import { normalize, slugify as slug } from './util';");
        let names: Vec<_> = out
            .imports
            .iter()
            .map(|i| (i.imported.as_str(), i.local.as_str(), i.module.as_str()))
            .collect();
        assert!(names.contains(&("normalize", "normalize", "./util")));
        assert!(names.contains(&("slugify", "slug", "./util")));
    }

    #[test]
    fn default_and_namespace_imports() {
        let out = extract("import React from 'react';\nimport * as fs from 'fs';");
        assert!(out
            .imports
            .iter()
            .any(|i| i.local == "React" && i.imported == "default"));
        assert!(out
            .imports
            .iter()
            .any(|i| i.local == "fs" && i.imported == "*"));
    }

    #[test]
    fn this_calls_are_self_receivers() {
        let out = extract("class Cache { a() {} b() { this.a(); } }");
        let call = out.refs.iter().find(|r| r.name == "a").unwrap();
        assert_eq!(call.receiver, Some(Receiver::SelfType));
    }

    #[test]
    fn member_calls_record_their_object() {
        let out = extract("function f(cache) { cache.get('k'); }");
        let call = out.refs.iter().find(|r| r.name == "get").unwrap();
        assert_eq!(call.receiver, Some(Receiver::Untyped("cache".to_string())));
    }

    #[test]
    fn new_expressions_reference_the_class() {
        let out = extract("function f() { return new Cache(); }");
        assert!(out
            .refs
            .iter()
            .any(|r| r.name == "Cache" && r.kind == EdgeKind::Calls));
    }

    #[test]
    fn annotated_and_constructed_variables_bind_types() {
        let out = extract("const a: Cache = x;\nconst b = new Cache();");
        assert!(out
            .bindings
            .contains(&("a".to_string(), "Cache".to_string())));
        assert!(out
            .bindings
            .contains(&("b".to_string(), "Cache".to_string())));
    }

    #[test]
    fn generic_types_reduce_to_their_base() {
        assert_eq!(base_type_name("Array<Cache>"), "Array");
        assert_eq!(base_type_name("Cache[]"), "Cache");
        assert_eq!(base_type_name("  Cache  "), "Cache");
    }

    #[test]
    fn tsx_files_parse() {
        let out = extract_file(
            "c.tsx",
            "export const C = () => <div>hi</div>;",
            Language::Tsx,
        )
        .unwrap()
        .unwrap();
        assert!(out.nodes.iter().any(|n| n.name == "C"));
    }

    #[test]
    fn plain_javascript_parses() {
        let out = extract_file(
            "c.js",
            "export function f(a) { return a; }",
            Language::JavaScript,
        )
        .unwrap()
        .unwrap();
        assert!(out.nodes.iter().any(|n| n.name == "f"));
    }
}
