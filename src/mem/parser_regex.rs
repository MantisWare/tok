//! Regex-based symbol extraction for common programming languages.
//!
//! Extracts functions, classes, structs, enums, traits, interfaces, imports,
//! and approximate call edges using language-aware regex patterns.
//! Covers the 80% case; tree-sitter (feature-gated) provides higher accuracy.

use lazy_static::lazy_static;
use regex::Regex;

use super::symbols::{generate_symbol_id, Edge, EdgeType, Symbol, SymbolKind};

/// Result of parsing a single source file.
#[derive(Debug, Default)]
pub struct ParseResult {
    pub symbols: Vec<Symbol>,
    pub edges: Vec<Edge>,
}

/// Detected source language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Rust,
    TypeScript,
    JavaScript,
    Python,
    Go,
    Ruby,
    CSharp,
    Java,
    Unknown,
}

impl Language {
    pub fn from_extension(ext: &str) -> Self {
        match ext {
            "rs" => Self::Rust,
            "ts" | "tsx" => Self::TypeScript,
            "js" | "jsx" | "mjs" | "cjs" => Self::JavaScript,
            "py" | "pyi" => Self::Python,
            "go" => Self::Go,
            "rb" => Self::Ruby,
            "cs" => Self::CSharp,
            "java" => Self::Java,
            _ => Self::Unknown,
        }
    }
}

/// Parse a source file and extract symbols + edges.
pub fn parse_file(content: &str, file_path: &str, repo_id: &str, branch: &str) -> ParseResult {
    let ext = file_path.rsplit('.').next().unwrap_or("");
    let lang = Language::from_extension(ext);

    match lang {
        Language::Rust => parse_rust(content, file_path, repo_id, branch),
        Language::TypeScript | Language::JavaScript => {
            parse_typescript(content, file_path, repo_id, branch)
        }
        Language::Python => parse_python(content, file_path, repo_id, branch),
        Language::Go => parse_go(content, file_path, repo_id, branch),
        Language::Ruby => parse_ruby(content, file_path, repo_id, branch),
        Language::CSharp | Language::Java => parse_clike(content, file_path, repo_id, branch),
        Language::Unknown => ParseResult::default(),
    }
}

/// Which file extensions we support for indexing.
pub fn is_supported_extension(ext: &str) -> bool {
    !matches!(Language::from_extension(ext), Language::Unknown)
}

// ── Rust ──

lazy_static! {
    static ref RUST_FN: Regex = Regex::new(
        r"(?m)^[ \t]*(?:pub(?:\(crate\))?\s+)?(?:async\s+)?fn\s+(\w+)\s*(?:<[^>]*>)?\s*\(([^)]*)\)(?:\s*->\s*([^\{]+))?"
    ).unwrap();

    static ref RUST_STRUCT: Regex = Regex::new(
        r"(?m)^[ \t]*(?:pub(?:\(crate\))?\s+)?struct\s+(\w+)"
    ).unwrap();

    static ref RUST_ENUM: Regex = Regex::new(
        r"(?m)^[ \t]*(?:pub(?:\(crate\))?\s+)?enum\s+(\w+)"
    ).unwrap();

    static ref RUST_TRAIT: Regex = Regex::new(
        r"(?m)^[ \t]*(?:pub(?:\(crate\))?\s+)?trait\s+(\w+)"
    ).unwrap();

    static ref RUST_IMPL: Regex = Regex::new(
        r"(?m)^[ \t]*impl(?:<[^>]*>)?\s+(\w+)\s+for\s+(\w+)"
    ).unwrap();

    static ref RUST_USE: Regex = Regex::new(
        r"(?m)^[ \t]*use\s+([^;]+);"
    ).unwrap();

    static ref RUST_CONST: Regex = Regex::new(
        r"(?m)^[ \t]*(?:pub(?:\(crate\))?\s+)?(?:const|static)\s+(\w+)\s*:"
    ).unwrap();

    static ref RUST_TYPE_ALIAS: Regex = Regex::new(
        r"(?m)^[ \t]*(?:pub(?:\(crate\))?\s+)?type\s+(\w+)"
    ).unwrap();

    static ref RUST_MOD: Regex = Regex::new(
        r"(?m)^[ \t]*(?:pub(?:\(crate\))?\s+)?mod\s+(\w+)"
    ).unwrap();
}

fn parse_rust(content: &str, file_path: &str, repo_id: &str, branch: &str) -> ParseResult {
    let mut result = ParseResult::default();
    let now = chrono::Utc::now().to_rfc3339();
    let lines: Vec<&str> = content.lines().collect();

    // Extract doc comments preceding a symbol (look back from line)
    let doc_at = |line_idx: usize| -> String {
        let mut docs = Vec::new();
        let mut i = line_idx;
        while i > 0 {
            i -= 1;
            let trimmed = lines.get(i).map(|l| l.trim()).unwrap_or("");
            if trimmed.starts_with("///") || trimmed.starts_with("//!") {
                docs.push(
                    trimmed
                        .trim_start_matches("///")
                        .trim_start_matches("//!")
                        .trim(),
                );
            } else if trimmed.starts_with("#[") || trimmed.is_empty() {
                // attributes or blank lines between doc and symbol
                continue;
            } else {
                break;
            }
        }
        docs.reverse();
        docs.join(" ").trim().to_string()
    };

    let line_of =
        |byte_offset: usize| -> u32 { content[..byte_offset].matches('\n').count() as u32 + 1 };

    for cap in RUST_FN.captures_iter(content) {
        let name = &cap[1];
        let params = cap.get(2).map(|m| m.as_str()).unwrap_or("");
        let ret = cap.get(3).map(|m| m.as_str().trim()).unwrap_or("");
        let line = line_of(cap.get(0).unwrap().start());
        let sig = if ret.is_empty() {
            format!("fn {}({})", name, params.trim())
        } else {
            format!("fn {}({}) -> {}", name, params.trim(), ret.trim())
        };

        result.symbols.push(Symbol {
            id: generate_symbol_id(repo_id, file_path, name, SymbolKind::Function),
            repo_id: repo_id.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Function,
            file_path: file_path.to_string(),
            line_start: line,
            line_end: line,
            signature: sig,
            doc_comment: doc_at(line as usize - 1),
            branch: branch.to_string(),
            indexed_at: now.clone(),
        });
    }

    for cap in RUST_STRUCT.captures_iter(content) {
        let name = &cap[1];
        let line = line_of(cap.get(0).unwrap().start());
        result.symbols.push(Symbol {
            id: generate_symbol_id(repo_id, file_path, name, SymbolKind::Struct),
            repo_id: repo_id.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Struct,
            file_path: file_path.to_string(),
            line_start: line,
            line_end: line,
            signature: format!("struct {}", name),
            doc_comment: doc_at(line as usize - 1),
            branch: branch.to_string(),
            indexed_at: now.clone(),
        });
    }

    for cap in RUST_ENUM.captures_iter(content) {
        let name = &cap[1];
        let line = line_of(cap.get(0).unwrap().start());
        result.symbols.push(Symbol {
            id: generate_symbol_id(repo_id, file_path, name, SymbolKind::Enum),
            repo_id: repo_id.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Enum,
            file_path: file_path.to_string(),
            line_start: line,
            line_end: line,
            signature: format!("enum {}", name),
            doc_comment: doc_at(line as usize - 1),
            branch: branch.to_string(),
            indexed_at: now.clone(),
        });
    }

    for cap in RUST_TRAIT.captures_iter(content) {
        let name = &cap[1];
        let line = line_of(cap.get(0).unwrap().start());
        result.symbols.push(Symbol {
            id: generate_symbol_id(repo_id, file_path, name, SymbolKind::Trait),
            repo_id: repo_id.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Trait,
            file_path: file_path.to_string(),
            line_start: line,
            line_end: line,
            signature: format!("trait {}", name),
            doc_comment: doc_at(line as usize - 1),
            branch: branch.to_string(),
            indexed_at: now.clone(),
        });
    }

    for cap in RUST_CONST.captures_iter(content) {
        let name = &cap[1];
        let line = line_of(cap.get(0).unwrap().start());
        result.symbols.push(Symbol {
            id: generate_symbol_id(repo_id, file_path, name, SymbolKind::Const),
            repo_id: repo_id.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Const,
            file_path: file_path.to_string(),
            line_start: line,
            line_end: line,
            signature: name.to_string(),
            doc_comment: doc_at(line as usize - 1),
            branch: branch.to_string(),
            indexed_at: now.clone(),
        });
    }

    for cap in RUST_TYPE_ALIAS.captures_iter(content) {
        let name = &cap[1];
        let line = line_of(cap.get(0).unwrap().start());
        result.symbols.push(Symbol {
            id: generate_symbol_id(repo_id, file_path, name, SymbolKind::Type),
            repo_id: repo_id.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Type,
            file_path: file_path.to_string(),
            line_start: line,
            line_end: line,
            signature: format!("type {}", name),
            doc_comment: doc_at(line as usize - 1),
            branch: branch.to_string(),
            indexed_at: now.clone(),
        });
    }

    for cap in RUST_MOD.captures_iter(content) {
        let name = &cap[1];
        let line = line_of(cap.get(0).unwrap().start());
        result.symbols.push(Symbol {
            id: generate_symbol_id(repo_id, file_path, name, SymbolKind::Module),
            repo_id: repo_id.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Module,
            file_path: file_path.to_string(),
            line_start: line,
            line_end: line,
            signature: format!("mod {}", name),
            doc_comment: String::new(),
            branch: branch.to_string(),
            indexed_at: now.clone(),
        });
    }

    // Edges: impl Trait for Struct → IMPLEMENTS edge
    for cap in RUST_IMPL.captures_iter(content) {
        let trait_name = &cap[1];
        let struct_name = &cap[2];
        let trait_id = generate_symbol_id(repo_id, file_path, trait_name, SymbolKind::Trait);
        let struct_id = generate_symbol_id(repo_id, file_path, struct_name, SymbolKind::Struct);
        result.edges.push(Edge {
            source_id: struct_id,
            target_id: trait_id,
            edge_type: EdgeType::Implements,
            repo_id: repo_id.to_string(),
            branch: branch.to_string(),
        });
    }

    // Edges: use statements → IMPORTS edges
    for cap in RUST_USE.captures_iter(content) {
        let path = cap[1].trim();
        if let Some(last_segment) = path.rsplit("::").next() {
            let imported_name =
                last_segment.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
            if !imported_name.is_empty() && imported_name != "*" && imported_name != "self" {
                let import_sym_id = generate_symbol_id(
                    repo_id,
                    file_path,
                    &format!("use_{}", imported_name),
                    SymbolKind::Import,
                );
                result.symbols.push(Symbol {
                    id: import_sym_id.clone(),
                    repo_id: repo_id.to_string(),
                    name: format!("use {}", path),
                    kind: SymbolKind::Import,
                    file_path: file_path.to_string(),
                    line_start: line_of(cap.get(0).unwrap().start()),
                    line_end: line_of(cap.get(0).unwrap().start()),
                    signature: format!("use {}", path),
                    doc_comment: String::new(),
                    branch: branch.to_string(),
                    indexed_at: now.clone(),
                });
            }
        }
    }

    result
}

// ── TypeScript / JavaScript ──

lazy_static! {
    static ref TS_FUNCTION: Regex = Regex::new(
        r"(?m)^[ \t]*(?:export\s+)?(?:async\s+)?function\s+(\w+)\s*(?:<[^>]*>)?\s*\(([^)]*)\)"
    ).unwrap();

    static ref TS_ARROW: Regex = Regex::new(
        r"(?m)^[ \t]*(?:export\s+)?(?:const|let|var)\s+(\w+)\s*(?::\s*[^=]+)?\s*=\s*(?:async\s+)?(?:\([^)]*\)|[a-zA-Z_]\w*)\s*(?::\s*[^=]+)?\s*=>"
    ).unwrap();

    static ref TS_CLASS: Regex = Regex::new(
        r"(?m)^[ \t]*(?:export\s+)?(?:abstract\s+)?class\s+(\w+)(?:\s+extends\s+(\w+))?(?:\s+implements\s+([^{]+))?"
    ).unwrap();

    static ref TS_INTERFACE: Regex = Regex::new(
        r"(?m)^[ \t]*(?:export\s+)?interface\s+(\w+)"
    ).unwrap();

    static ref TS_TYPE: Regex = Regex::new(
        r"(?m)^[ \t]*(?:export\s+)?type\s+(\w+)"
    ).unwrap();

    static ref TS_IMPORT: Regex = Regex::new(
        r#"(?m)^[ \t]*import\s+(?:\{[^}]*\}|[^;]+)\s+from\s+['"]([^'"]+)['"]"#
    ).unwrap();

    static ref TS_METHOD: Regex = Regex::new(
        r"(?m)^[ \t]+(?:async\s+)?(?:static\s+)?(?:get\s+|set\s+)?(\w+)\s*\([^)]*\)\s*(?::\s*[^{]+)?\s*\{"
    ).unwrap();
}

fn parse_typescript(content: &str, file_path: &str, repo_id: &str, branch: &str) -> ParseResult {
    let mut result = ParseResult::default();
    let now = chrono::Utc::now().to_rfc3339();

    let line_of =
        |byte_offset: usize| -> u32 { content[..byte_offset].matches('\n').count() as u32 + 1 };

    for cap in TS_FUNCTION.captures_iter(content) {
        let name = &cap[1];
        let params = cap.get(2).map(|m| m.as_str()).unwrap_or("");
        let line = line_of(cap.get(0).unwrap().start());
        result.symbols.push(Symbol {
            id: generate_symbol_id(repo_id, file_path, name, SymbolKind::Function),
            repo_id: repo_id.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Function,
            file_path: file_path.to_string(),
            line_start: line,
            line_end: line,
            signature: format!("function {}({})", name, params.trim()),
            doc_comment: String::new(),
            branch: branch.to_string(),
            indexed_at: now.clone(),
        });
    }

    for cap in TS_ARROW.captures_iter(content) {
        let name = &cap[1];
        let line = line_of(cap.get(0).unwrap().start());
        result.symbols.push(Symbol {
            id: generate_symbol_id(repo_id, file_path, name, SymbolKind::Function),
            repo_id: repo_id.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Function,
            file_path: file_path.to_string(),
            line_start: line,
            line_end: line,
            signature: format!("const {} = (...) =>", name),
            doc_comment: String::new(),
            branch: branch.to_string(),
            indexed_at: now.clone(),
        });
    }

    for cap in TS_CLASS.captures_iter(content) {
        let name = &cap[1];
        let line = line_of(cap.get(0).unwrap().start());
        let extends = cap.get(2).map(|m| m.as_str());
        let sig = match extends {
            Some(parent) => format!("class {} extends {}", name, parent),
            None => format!("class {}", name),
        };
        result.symbols.push(Symbol {
            id: generate_symbol_id(repo_id, file_path, name, SymbolKind::Class),
            repo_id: repo_id.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Class,
            file_path: file_path.to_string(),
            line_start: line,
            line_end: line,
            signature: sig,
            doc_comment: String::new(),
            branch: branch.to_string(),
            indexed_at: now.clone(),
        });

        // IMPLEMENTS edges for "implements" clause
        if let Some(implements) = cap.get(3) {
            for iface in implements.as_str().split(',') {
                let iface = iface.trim();
                if !iface.is_empty() {
                    let class_id = generate_symbol_id(repo_id, file_path, name, SymbolKind::Class);
                    let iface_id =
                        generate_symbol_id(repo_id, file_path, iface, SymbolKind::Interface);
                    result.edges.push(Edge {
                        source_id: class_id,
                        target_id: iface_id,
                        edge_type: EdgeType::Implements,
                        repo_id: repo_id.to_string(),
                        branch: branch.to_string(),
                    });
                }
            }
        }
    }

    for cap in TS_INTERFACE.captures_iter(content) {
        let name = &cap[1];
        let line = line_of(cap.get(0).unwrap().start());
        result.symbols.push(Symbol {
            id: generate_symbol_id(repo_id, file_path, name, SymbolKind::Interface),
            repo_id: repo_id.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Interface,
            file_path: file_path.to_string(),
            line_start: line,
            line_end: line,
            signature: format!("interface {}", name),
            doc_comment: String::new(),
            branch: branch.to_string(),
            indexed_at: now.clone(),
        });
    }

    for cap in TS_TYPE.captures_iter(content) {
        let name = &cap[1];
        let line = line_of(cap.get(0).unwrap().start());
        result.symbols.push(Symbol {
            id: generate_symbol_id(repo_id, file_path, name, SymbolKind::Type),
            repo_id: repo_id.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Type,
            file_path: file_path.to_string(),
            line_start: line,
            line_end: line,
            signature: format!("type {}", name),
            doc_comment: String::new(),
            branch: branch.to_string(),
            indexed_at: now.clone(),
        });
    }

    for cap in TS_IMPORT.captures_iter(content) {
        let module = &cap[1];
        let line = line_of(cap.get(0).unwrap().start());
        result.symbols.push(Symbol {
            id: generate_symbol_id(
                repo_id,
                file_path,
                &format!("import_{}", module),
                SymbolKind::Import,
            ),
            repo_id: repo_id.to_string(),
            name: format!("import {}", module),
            kind: SymbolKind::Import,
            file_path: file_path.to_string(),
            line_start: line,
            line_end: line,
            signature: cap[0].trim().to_string(),
            doc_comment: String::new(),
            branch: branch.to_string(),
            indexed_at: now.clone(),
        });
    }

    result
}

// ── Python ──

lazy_static! {
    static ref PY_FUNCTION: Regex =
        Regex::new(r"(?m)^[ \t]*(?:async\s+)?def\s+(\w+)\s*\(([^)]*)\)(?:\s*->\s*([^:]+))?")
            .unwrap();
    static ref PY_CLASS: Regex = Regex::new(r"(?m)^class\s+(\w+)(?:\(([^)]*)\))?").unwrap();
    static ref PY_IMPORT: Regex = Regex::new(r"(?m)^(?:from\s+(\S+)\s+)?import\s+(.+)").unwrap();
}

fn parse_python(content: &str, file_path: &str, repo_id: &str, branch: &str) -> ParseResult {
    let mut result = ParseResult::default();
    let now = chrono::Utc::now().to_rfc3339();
    let lines: Vec<&str> = content.lines().collect();

    let line_of =
        |byte_offset: usize| -> u32 { content[..byte_offset].matches('\n').count() as u32 + 1 };

    let docstring_at = |line_idx: usize| -> String {
        if line_idx + 1 < lines.len() {
            let next = lines[line_idx + 1].trim();
            if next.starts_with("\"\"\"") || next.starts_with("'''") {
                let quote = &next[..3];
                if next.len() > 6 && next.ends_with(quote) {
                    return next[3..next.len() - 3].trim().to_string();
                }
            }
        }
        String::new()
    };

    for cap in PY_FUNCTION.captures_iter(content) {
        let name = &cap[1];
        let params = cap.get(2).map(|m| m.as_str()).unwrap_or("");
        let ret = cap.get(3).map(|m| m.as_str()).unwrap_or("");
        let line = line_of(cap.get(0).unwrap().start());
        let first_param = params.split(',').next().unwrap_or("").trim();
        let kind = if first_param == "self"
            || first_param == "cls"
            || first_param.starts_with("self:")
            || first_param.starts_with("cls:")
        {
            SymbolKind::Method
        } else {
            SymbolKind::Function
        };

        let sig = if ret.is_empty() {
            format!("def {}({})", name, params.trim())
        } else {
            format!("def {}({}) -> {}", name, params.trim(), ret)
        };

        result.symbols.push(Symbol {
            id: generate_symbol_id(repo_id, file_path, name, kind),
            repo_id: repo_id.to_string(),
            name: name.to_string(),
            kind,
            file_path: file_path.to_string(),
            line_start: line,
            line_end: line,
            signature: sig,
            doc_comment: docstring_at(line as usize - 1),
            branch: branch.to_string(),
            indexed_at: now.clone(),
        });
    }

    for cap in PY_CLASS.captures_iter(content) {
        let name = &cap[1];
        let bases = cap.get(2).map(|m| m.as_str()).unwrap_or("");
        let line = line_of(cap.get(0).unwrap().start());
        let sig = if bases.is_empty() {
            format!("class {}", name)
        } else {
            format!("class {}({})", name, bases)
        };

        result.symbols.push(Symbol {
            id: generate_symbol_id(repo_id, file_path, name, SymbolKind::Class),
            repo_id: repo_id.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Class,
            file_path: file_path.to_string(),
            line_start: line,
            line_end: line,
            signature: sig,
            doc_comment: docstring_at(line as usize - 1),
            branch: branch.to_string(),
            indexed_at: now.clone(),
        });

        // IMPLEMENTS edges for base classes
        if !bases.is_empty() {
            for base in bases.split(',') {
                let base = base.trim();
                if !base.is_empty() && base != "object" {
                    let class_id = generate_symbol_id(repo_id, file_path, name, SymbolKind::Class);
                    let base_id = generate_symbol_id(repo_id, file_path, base, SymbolKind::Class);
                    result.edges.push(Edge {
                        source_id: class_id,
                        target_id: base_id,
                        edge_type: EdgeType::Implements,
                        repo_id: repo_id.to_string(),
                        branch: branch.to_string(),
                    });
                }
            }
        }
    }

    for cap in PY_IMPORT.captures_iter(content) {
        let module = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let names = &cap[2];
        let line = line_of(cap.get(0).unwrap().start());
        let label = if module.is_empty() {
            format!("import {}", names.trim())
        } else {
            format!("from {} import {}", module, names.trim())
        };
        result.symbols.push(Symbol {
            id: generate_symbol_id(
                repo_id,
                file_path,
                &format!("import_{}", label.replace(' ', "_")),
                SymbolKind::Import,
            ),
            repo_id: repo_id.to_string(),
            name: label.clone(),
            kind: SymbolKind::Import,
            file_path: file_path.to_string(),
            line_start: line,
            line_end: line,
            signature: label,
            doc_comment: String::new(),
            branch: branch.to_string(),
            indexed_at: now.clone(),
        });
    }

    result
}

// ── Go ──

lazy_static! {
    static ref GO_FUNC: Regex = Regex::new(
        r"(?m)^func\s+(?:\(\s*\w+\s+\*?\w+\s*\)\s+)?(\w+)\s*\(([^)]*)\)(?:\s*(?:\([^)]*\)|\S+))?"
    )
    .unwrap();
    static ref GO_TYPE: Regex = Regex::new(r"(?m)^type\s+(\w+)\s+(struct|interface)").unwrap();
    static ref GO_IMPORT: Regex = Regex::new(r#"(?m)^\s*"([^"]+)""#).unwrap();
}

fn parse_go(content: &str, file_path: &str, repo_id: &str, branch: &str) -> ParseResult {
    let mut result = ParseResult::default();
    let now = chrono::Utc::now().to_rfc3339();

    let line_of =
        |byte_offset: usize| -> u32 { content[..byte_offset].matches('\n').count() as u32 + 1 };

    for cap in GO_FUNC.captures_iter(content) {
        let name = &cap[1];
        let params = cap.get(2).map(|m| m.as_str()).unwrap_or("");
        let line = line_of(cap.get(0).unwrap().start());
        let full_match = cap.get(0).unwrap().as_str();
        let is_method = full_match.contains("(") && full_match.starts_with("func (");
        let kind = if is_method {
            SymbolKind::Method
        } else {
            SymbolKind::Function
        };

        result.symbols.push(Symbol {
            id: generate_symbol_id(repo_id, file_path, name, kind),
            repo_id: repo_id.to_string(),
            name: name.to_string(),
            kind,
            file_path: file_path.to_string(),
            line_start: line,
            line_end: line,
            signature: format!("func {}({})", name, params.trim()),
            doc_comment: String::new(),
            branch: branch.to_string(),
            indexed_at: now.clone(),
        });
    }

    for cap in GO_TYPE.captures_iter(content) {
        let name = &cap[1];
        let kind_str = &cap[2];
        let line = line_of(cap.get(0).unwrap().start());
        let kind = if kind_str == "interface" {
            SymbolKind::Interface
        } else {
            SymbolKind::Struct
        };

        result.symbols.push(Symbol {
            id: generate_symbol_id(repo_id, file_path, name, kind),
            repo_id: repo_id.to_string(),
            name: name.to_string(),
            kind,
            file_path: file_path.to_string(),
            line_start: line,
            line_end: line,
            signature: format!("type {} {}", name, kind_str),
            doc_comment: String::new(),
            branch: branch.to_string(),
            indexed_at: now.clone(),
        });
    }

    result
}

// ── Ruby ──

lazy_static! {
    static ref RUBY_DEF: Regex = Regex::new(r"(?m)^[ \t]*def\s+(self\.)?(\w+[?!=]?)").unwrap();
    static ref RUBY_CLASS: Regex =
        Regex::new(r"(?m)^[ \t]*class\s+(\w+)(?:\s*<\s*(\w+))?").unwrap();
    static ref RUBY_MODULE: Regex = Regex::new(r"(?m)^[ \t]*module\s+(\w+)").unwrap();
}

fn parse_ruby(content: &str, file_path: &str, repo_id: &str, branch: &str) -> ParseResult {
    let mut result = ParseResult::default();
    let now = chrono::Utc::now().to_rfc3339();

    let line_of =
        |byte_offset: usize| -> u32 { content[..byte_offset].matches('\n').count() as u32 + 1 };

    for cap in RUBY_DEF.captures_iter(content) {
        let is_class_method = cap.get(1).is_some();
        let name = &cap[2];
        let line = line_of(cap.get(0).unwrap().start());
        let kind = if is_class_method {
            SymbolKind::Function
        } else {
            SymbolKind::Method
        };
        result.symbols.push(Symbol {
            id: generate_symbol_id(repo_id, file_path, name, kind),
            repo_id: repo_id.to_string(),
            name: name.to_string(),
            kind,
            file_path: file_path.to_string(),
            line_start: line,
            line_end: line,
            signature: format!("def {}", name),
            doc_comment: String::new(),
            branch: branch.to_string(),
            indexed_at: now.clone(),
        });
    }

    for cap in RUBY_CLASS.captures_iter(content) {
        let name = &cap[1];
        let line = line_of(cap.get(0).unwrap().start());
        result.symbols.push(Symbol {
            id: generate_symbol_id(repo_id, file_path, name, SymbolKind::Class),
            repo_id: repo_id.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Class,
            file_path: file_path.to_string(),
            line_start: line,
            line_end: line,
            signature: format!("class {}", name),
            doc_comment: String::new(),
            branch: branch.to_string(),
            indexed_at: now.clone(),
        });
    }

    for cap in RUBY_MODULE.captures_iter(content) {
        let name = &cap[1];
        let line = line_of(cap.get(0).unwrap().start());
        result.symbols.push(Symbol {
            id: generate_symbol_id(repo_id, file_path, name, SymbolKind::Module),
            repo_id: repo_id.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Module,
            file_path: file_path.to_string(),
            line_start: line,
            line_end: line,
            signature: format!("module {}", name),
            doc_comment: String::new(),
            branch: branch.to_string(),
            indexed_at: now.clone(),
        });
    }

    result
}

// ── C# / Java (shared C-like patterns) ──

lazy_static! {
    static ref CLIKE_CLASS: Regex = Regex::new(
        r"(?m)^[ \t]*(?:public|private|protected|internal|abstract|sealed|static)?\s*(?:partial\s+)?class\s+(\w+)"
    ).unwrap();

    static ref CLIKE_INTERFACE: Regex = Regex::new(
        r"(?m)^[ \t]*(?:public|private|protected|internal)?\s*interface\s+(\w+)"
    ).unwrap();

    static ref CLIKE_METHOD: Regex = Regex::new(
        r"(?m)^[ \t]+(?:public|private|protected|internal|static|virtual|override|abstract|async)?\s*(?:\w+(?:<[^>]+>)?)\s+(\w+)\s*\([^)]*\)"
    ).unwrap();
}

fn parse_clike(content: &str, file_path: &str, repo_id: &str, branch: &str) -> ParseResult {
    let mut result = ParseResult::default();
    let now = chrono::Utc::now().to_rfc3339();

    let line_of =
        |byte_offset: usize| -> u32 { content[..byte_offset].matches('\n').count() as u32 + 1 };

    for cap in CLIKE_CLASS.captures_iter(content) {
        let name = &cap[1];
        let line = line_of(cap.get(0).unwrap().start());
        result.symbols.push(Symbol {
            id: generate_symbol_id(repo_id, file_path, name, SymbolKind::Class),
            repo_id: repo_id.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Class,
            file_path: file_path.to_string(),
            line_start: line,
            line_end: line,
            signature: format!("class {}", name),
            doc_comment: String::new(),
            branch: branch.to_string(),
            indexed_at: now.clone(),
        });
    }

    for cap in CLIKE_INTERFACE.captures_iter(content) {
        let name = &cap[1];
        let line = line_of(cap.get(0).unwrap().start());
        result.symbols.push(Symbol {
            id: generate_symbol_id(repo_id, file_path, name, SymbolKind::Interface),
            repo_id: repo_id.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Interface,
            file_path: file_path.to_string(),
            line_start: line,
            line_end: line,
            signature: format!("interface {}", name),
            doc_comment: String::new(),
            branch: branch.to_string(),
            indexed_at: now.clone(),
        });
    }

    for cap in CLIKE_METHOD.captures_iter(content) {
        let name = &cap[1];
        let line = line_of(cap.get(0).unwrap().start());
        result.symbols.push(Symbol {
            id: generate_symbol_id(repo_id, file_path, name, SymbolKind::Method),
            repo_id: repo_id.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Method,
            file_path: file_path.to_string(),
            line_start: line,
            line_end: line,
            signature: cap[0].trim().to_string(),
            doc_comment: String::new(),
            branch: branch.to_string(),
            indexed_at: now.clone(),
        });
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_functions() {
        let src = r#"
/// Does something important
pub fn do_thing(x: u32) -> bool {
    true
}

fn helper() {
}

pub async fn fetch_data(url: &str) -> Result<String> {
    todo!()
}
"#;
        let r = parse_rust(src, "src/lib.rs", "test", "main");
        assert_eq!(
            r.symbols
                .iter()
                .filter(|s| s.kind == SymbolKind::Function)
                .count(),
            3
        );
        let do_thing = r.symbols.iter().find(|s| s.name == "do_thing").unwrap();
        assert!(do_thing.signature.contains("-> bool"));
        assert!(do_thing.doc_comment.contains("important"));
    }

    #[test]
    fn rust_structs_enums_traits() {
        let src = r#"
pub struct Config {
    name: String,
}

enum Color {
    Red,
    Blue,
}

pub trait Drawable {
    fn draw(&self);
}
"#;
        let r = parse_rust(src, "f.rs", "r", "main");
        assert!(r
            .symbols
            .iter()
            .any(|s| s.name == "Config" && s.kind == SymbolKind::Struct));
        assert!(r
            .symbols
            .iter()
            .any(|s| s.name == "Color" && s.kind == SymbolKind::Enum));
        assert!(r
            .symbols
            .iter()
            .any(|s| s.name == "Drawable" && s.kind == SymbolKind::Trait));
    }

    #[test]
    fn rust_impl_generates_edge() {
        let src = "impl Display for Config {}";
        let r = parse_rust(src, "f.rs", "r", "main");
        assert_eq!(r.edges.len(), 1);
        assert_eq!(r.edges[0].edge_type, EdgeType::Implements);
    }

    #[test]
    fn typescript_functions_and_classes() {
        let src = r#"
export function handleLogin(email: string, password: string): Promise<User> {
}

const fetchUser = async (id: string) => {
};

export class AuthService extends BaseService implements Authenticatable {
}

interface User {
    id: string;
}

type UserID = string;

import { useState } from 'react';
"#;
        let r = parse_typescript(src, "auth.ts", "r", "main");
        assert!(r
            .symbols
            .iter()
            .any(|s| s.name == "handleLogin" && s.kind == SymbolKind::Function));
        assert!(r
            .symbols
            .iter()
            .any(|s| s.name == "fetchUser" && s.kind == SymbolKind::Function));
        assert!(r
            .symbols
            .iter()
            .any(|s| s.name == "AuthService" && s.kind == SymbolKind::Class));
        assert!(r
            .symbols
            .iter()
            .any(|s| s.name == "User" && s.kind == SymbolKind::Interface));
        assert!(r
            .symbols
            .iter()
            .any(|s| s.name == "UserID" && s.kind == SymbolKind::Type));
        assert!(r.symbols.iter().any(|s| s.kind == SymbolKind::Import));
        // implements edge
        assert!(r.edges.iter().any(|e| e.edge_type == EdgeType::Implements));
    }

    #[test]
    fn python_functions_and_classes() {
        let src = r#"
def process_data(items: list) -> dict:
    """Process incoming data items."""
    pass

class DataPipeline(BasePipeline):
    def run(self):
        pass

from typing import List
import os
"#;
        let r = parse_python(src, "pipeline.py", "r", "main");
        assert!(r
            .symbols
            .iter()
            .any(|s| s.name == "process_data" && s.kind == SymbolKind::Function));
        assert!(r
            .symbols
            .iter()
            .any(|s| s.name == "DataPipeline" && s.kind == SymbolKind::Class));
        assert!(r
            .symbols
            .iter()
            .any(|s| s.name == "run" && s.kind == SymbolKind::Method));
        // base class edge
        assert!(r.edges.iter().any(|e| e.edge_type == EdgeType::Implements));
    }

    #[test]
    fn go_funcs_and_types() {
        let src = r#"
func HandleRequest(w http.ResponseWriter, r *http.Request) {
}

func (s *Server) Start() error {
}

type Config struct {
    Port int
}

type Handler interface {
    Handle(r Request)
}
"#;
        let r = parse_go(src, "main.go", "r", "main");
        assert!(r
            .symbols
            .iter()
            .any(|s| s.name == "HandleRequest" && s.kind == SymbolKind::Function));
        assert!(r
            .symbols
            .iter()
            .any(|s| s.name == "Start" && s.kind == SymbolKind::Method));
        assert!(r
            .symbols
            .iter()
            .any(|s| s.name == "Config" && s.kind == SymbolKind::Struct));
        assert!(r
            .symbols
            .iter()
            .any(|s| s.name == "Handler" && s.kind == SymbolKind::Interface));
    }

    #[test]
    fn language_detection() {
        assert_eq!(Language::from_extension("rs"), Language::Rust);
        assert_eq!(Language::from_extension("ts"), Language::TypeScript);
        assert_eq!(Language::from_extension("tsx"), Language::TypeScript);
        assert_eq!(Language::from_extension("py"), Language::Python);
        assert_eq!(Language::from_extension("go"), Language::Go);
        assert_eq!(Language::from_extension("rb"), Language::Ruby);
        assert_eq!(Language::from_extension("cs"), Language::CSharp);
        assert_eq!(Language::from_extension("java"), Language::Java);
        assert_eq!(Language::from_extension("txt"), Language::Unknown);
    }

    #[test]
    fn unknown_extension_returns_empty() {
        let r = parse_file("hello world", "readme.txt", "r", "main");
        assert!(r.symbols.is_empty());
    }
}
