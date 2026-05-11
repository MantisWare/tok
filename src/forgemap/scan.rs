//! Per-file extraction — exports, dependencies, and ForgeMap header detection.
//!
//! Delegates to `mem::parser_regex` for AST-like symbol extraction and
//! `header::parse_header` for ForgeMap header detection.

use std::collections::BTreeMap;
use std::path::Path;

use crate::mem::parser_regex;
use crate::mem::symbols::{EdgeType, SymbolKind};

use super::collect::to_posix_rel;
use super::header::parse_header;
use super::types::{DepMap, FileInfo};

/// Scan a single source file, returning its `FileInfo`.
///
/// Uses `mem::parser_regex::parse_file` for exports and dependency extraction,
/// and scans the first 30 lines for ForgeMap header fields.
pub fn scan_file(abs_path: &Path, repo_root: &Path) -> FileInfo {
    let rel = to_posix_rel(abs_path, repo_root);
    let source = match std::fs::read_to_string(abs_path) {
        Ok(s) => normalize_lf(&s),
        Err(_) => {
            return FileInfo {
                abs_path: abs_path.to_path_buf(),
                rel,
                exports: Vec::new(),
                deps: BTreeMap::new(),
                header: None,
                has_forgemap: false,
                parseable: false,
            };
        }
    };

    let rel_str = rel.as_str();
    let parse_result = parser_regex::parse_file(&source, rel_str, "forgemap", "main");

    let exports = extract_exports(&parse_result);
    let deps = extract_deps(&parse_result, repo_root, abs_path);
    let ext = abs_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let header = parse_header(&source, ext);
    let has_forgemap = header.is_some();

    FileInfo {
        abs_path: abs_path.to_path_buf(),
        rel,
        exports,
        deps,
        header,
        has_forgemap,
        parseable: true,
    }
}

/// Extract export signatures from parser results.
fn extract_exports(result: &parser_regex::ParseResult) -> Vec<String> {
    let mut exports = Vec::new();
    for sym in &result.symbols {
        match sym.kind {
            SymbolKind::Function | SymbolKind::Method => {
                if !sym.signature.is_empty() {
                    exports.push(sym.signature.clone());
                } else {
                    exports.push(sym.name.clone());
                }
            }
            SymbolKind::Class
            | SymbolKind::Struct
            | SymbolKind::Enum
            | SymbolKind::Trait
            | SymbolKind::Interface
            | SymbolKind::Type
            | SymbolKind::Const
            | SymbolKind::Static => {
                exports.push(sym.name.clone());
            }
            SymbolKind::Export => {
                exports.push(sym.name.clone());
            }
            _ => {}
        }
    }
    exports.sort();
    exports.dedup();
    exports
}

/// Extract dependency map from parser edges (imports).
fn extract_deps(
    result: &parser_regex::ParseResult,
    _repo_root: &Path,
    _current_file: &Path,
) -> DepMap {
    let mut deps: DepMap = BTreeMap::new();
    for edge in &result.edges {
        if edge.edge_type == EdgeType::Imports {
            let target = &edge.target_id;
            // The target_id from parser_regex is a symbol hash; we extract the
            // source symbol name from the edge's source info instead.
            // For ForgeMap, we track file-level deps from Import symbols.
            let entry = deps.entry(target.clone()).or_default();
            if !edge.source_id.is_empty() && !entry.contains(&edge.source_id) {
                entry.push(edge.source_id.clone());
            }
        }
    }

    // Also gather from Import-kind symbols which carry the module path in their name.
    for sym in &result.symbols {
        if sym.kind == SymbolKind::Import {
            let module_path = &sym.name;
            // Skip bare specifiers (no relative path indicators).
            if !module_path.starts_with('.')
                && !module_path.starts_with('/')
                && !module_path.contains("::")
            {
                continue;
            }
            deps.entry(module_path.clone()).or_default();
        }
    }

    deps
}

/// Normalize CRLF and bare CR to LF (critical invariant R1).
pub fn normalize_lf(source: &str) -> String {
    source.replace("\r\n", "\n").replace('\r', "\n")
}
