//! Core data structures for the tok mem structural memory subsystem.

use serde::Serialize;
use sha2::{Digest, Sha256};

/// Kinds of symbols extracted from source code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Struct,
    Enum,
    Trait,
    Interface,
    Type,
    Const,
    Static,
    Module,
    Import,
    Export,
    APIEndpoint,
    APICall,
}

impl SymbolKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Function => "Function",
            Self::Method => "Method",
            Self::Class => "Class",
            Self::Struct => "Struct",
            Self::Enum => "Enum",
            Self::Trait => "Trait",
            Self::Interface => "Interface",
            Self::Type => "Type",
            Self::Const => "Const",
            Self::Static => "Static",
            Self::Module => "Module",
            Self::Import => "Import",
            Self::Export => "Export",
            Self::APIEndpoint => "APIEndpoint",
            Self::APICall => "APICall",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "Function" => Some(Self::Function),
            "Method" => Some(Self::Method),
            "Class" => Some(Self::Class),
            "Struct" => Some(Self::Struct),
            "Enum" => Some(Self::Enum),
            "Trait" => Some(Self::Trait),
            "Interface" => Some(Self::Interface),
            "Type" => Some(Self::Type),
            "Const" => Some(Self::Const),
            "Static" => Some(Self::Static),
            "Module" => Some(Self::Module),
            "Import" => Some(Self::Import),
            "Export" => Some(Self::Export),
            "APIEndpoint" => Some(Self::APIEndpoint),
            "APICall" => Some(Self::APICall),
            _ => None,
        }
    }
}

impl std::fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Types of directed edges between symbols.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum EdgeType {
    Calls,
    Implements,
    Imports,
    Exports,
    Contains,
    TypeRef,
}

impl EdgeType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Calls => "CALLS",
            Self::Implements => "IMPLEMENTS",
            Self::Imports => "IMPORTS",
            Self::Exports => "EXPORTS",
            Self::Contains => "CONTAINS",
            Self::TypeRef => "TYPE_REF",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "CALLS" => Some(Self::Calls),
            "IMPLEMENTS" => Some(Self::Implements),
            "IMPORTS" => Some(Self::Imports),
            "EXPORTS" => Some(Self::Exports),
            "CONTAINS" => Some(Self::Contains),
            "TYPE_REF" => Some(Self::TypeRef),
            _ => None,
        }
    }
}

impl std::fmt::Display for EdgeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Types of changes tracked in episodes (Phase 2: temporal engine).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[allow(dead_code)]
pub enum ChangeType {
    Added,
    Modified,
    Removed,
}

#[allow(dead_code)]
impl ChangeType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Modified => "modified",
            Self::Removed => "removed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "added" => Some(Self::Added),
            "modified" => Some(Self::Modified),
            "removed" => Some(Self::Removed),
            _ => None,
        }
    }
}

/// A code symbol (function, class, struct, etc.) extracted from source.
#[derive(Debug, Clone, Serialize)]
pub struct Symbol {
    pub id: String,
    pub repo_id: String,
    pub name: String,
    pub kind: SymbolKind,
    pub file_path: String,
    pub line_start: u32,
    pub line_end: u32,
    pub signature: String,
    pub doc_comment: String,
    pub branch: String,
    pub indexed_at: String,
}

/// A directed edge between two symbols.
#[derive(Debug, Clone, Serialize)]
pub struct Edge {
    pub source_id: String,
    pub target_id: String,
    pub edge_type: EdgeType,
    pub repo_id: String,
    pub branch: String,
}

/// A temporal change record for a symbol (Phase 2: temporal engine).
#[derive(Debug, Clone, Serialize)]
#[allow(dead_code)]
pub struct Episode {
    pub id: String,
    pub repo_id: String,
    pub symbol_id: String,
    pub change_type: ChangeType,
    pub commit_hash: String,
    pub timestamp: String,
    pub diff_summary: String,
    pub branch: String,
}

/// Metadata for an indexed repository.
#[derive(Debug, Clone, Serialize)]
pub struct Repository {
    pub repo_id: String,
    pub path: String,
    pub branch: String,
    pub last_indexed_at: String,
    pub last_episode_id: String,
    pub symbol_count: usize,
    pub edge_count: usize,
    pub file_count: usize,
}

/// A search result with BM25 rank score.
#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub symbol: Symbol,
    pub rank: f64,
}

/// Impact analysis result: a symbol and its graph distance from the origin.
#[derive(Debug, Clone, Serialize)]
pub struct ImpactNode {
    pub symbol: Symbol,
    pub depth: u32,
    pub edge_type: String,
}

/// Generate a deterministic ID from repo_id + file_path + name + kind.
pub fn generate_symbol_id(repo_id: &str, file_path: &str, name: &str, kind: SymbolKind) -> String {
    let mut hasher = Sha256::new();
    hasher.update(repo_id.as_bytes());
    hasher.update(b":");
    hasher.update(file_path.as_bytes());
    hasher.update(b":");
    hasher.update(name.as_bytes());
    hasher.update(b":");
    hasher.update(kind.as_str().as_bytes());
    let hash = hasher.finalize();
    format!("{:x}", hash)[..16].to_string()
}

/// Generate a deterministic episode ID from repo + symbol + commit + change_type.
#[allow(dead_code)]
pub fn generate_episode_id(
    repo_id: &str,
    symbol_id: &str,
    commit_hash: &str,
    change_type: ChangeType,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(repo_id.as_bytes());
    hasher.update(b":");
    hasher.update(symbol_id.as_bytes());
    hasher.update(b":");
    hasher.update(commit_hash.as_bytes());
    hasher.update(b":");
    hasher.update(change_type.as_str().as_bytes());
    let hash = hasher.finalize();
    format!("ep_{}", &format!("{:x}", hash)[..14])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_id_is_deterministic() {
        let id1 = generate_symbol_id("myrepo", "src/main.rs", "main", SymbolKind::Function);
        let id2 = generate_symbol_id("myrepo", "src/main.rs", "main", SymbolKind::Function);
        assert_eq!(id1, id2);
        assert_eq!(id1.len(), 16);
    }

    #[test]
    fn symbol_id_differs_by_kind() {
        let fn_id = generate_symbol_id("r", "f.rs", "Foo", SymbolKind::Function);
        let st_id = generate_symbol_id("r", "f.rs", "Foo", SymbolKind::Struct);
        assert_ne!(fn_id, st_id);
    }

    #[test]
    fn episode_id_is_deterministic() {
        let id1 = generate_episode_id("r", "sym1", "abc123", ChangeType::Modified);
        let id2 = generate_episode_id("r", "sym1", "abc123", ChangeType::Modified);
        assert_eq!(id1, id2);
        assert!(id1.starts_with("ep_"));
    }

    #[test]
    fn symbol_kind_roundtrip() {
        for kind in [
            SymbolKind::Function,
            SymbolKind::Struct,
            SymbolKind::Enum,
            SymbolKind::Trait,
            SymbolKind::Interface,
        ] {
            assert_eq!(SymbolKind::from_str(kind.as_str()), Some(kind));
        }
    }

    #[test]
    fn edge_type_roundtrip() {
        for et in [
            EdgeType::Calls,
            EdgeType::Implements,
            EdgeType::Imports,
            EdgeType::Contains,
        ] {
            assert_eq!(EdgeType::from_str(et.as_str()), Some(et));
        }
    }
}
