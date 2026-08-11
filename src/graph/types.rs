//! Serialized code-graph format (`GraphV1`).
//!
//! This is the on-disk contract for `.tok/graph/graph.json`. Two properties are
//! load-bearing and every producer must uphold them:
//!
//! 1. **Determinism** — the same tree on two machines must serialize to byte
//!    identical JSON. Nodes and edges are therefore sorted before writing, and
//!    no timestamp or absolute path is stored inside the graph itself.
//! 2. **Additive evolution** — readers ignore unknown fields, so a newer TOK can
//!    add node metadata without invalidating graphs written by an older one.
//!    A breaking change requires bumping [`GRAPH_FORMAT_VERSION`], which forces
//!    a rebuild instead of a misparse.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Bump only for changes that an older reader would misinterpret. Adding an
/// optional field is not such a change.
pub const GRAPH_FORMAT_VERSION: u32 = 1;

/// What a node represents in the source.
///
/// Deliberately coarser than any single language's AST: the retrieval layer
/// ranks across languages, so `Class` covers Python classes, TS classes, and Go
/// structs alike. Language-specific detail lives in [`NodeV1::signature`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    /// A whole source file. Always present, and the anchor for `CONTAINS`.
    File,
    Function,
    Method,
    Class,
    Interface,
    Struct,
    Enum,
    Trait,
    Type,
    Constant,
    Variable,
    Module,
    /// A named import binding, kept so `IMPORTS` edges can resolve across files.
    Import,
}

impl NodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            NodeKind::File => "file",
            NodeKind::Function => "function",
            NodeKind::Method => "method",
            NodeKind::Class => "class",
            NodeKind::Interface => "interface",
            NodeKind::Struct => "struct",
            NodeKind::Enum => "enum",
            NodeKind::Trait => "trait",
            NodeKind::Type => "type",
            NodeKind::Constant => "constant",
            NodeKind::Variable => "variable",
            NodeKind::Module => "module",
            NodeKind::Import => "import",
        }
    }

    /// Kinds that can own methods, used when resolving a typed receiver.
    pub fn is_type_like(self) -> bool {
        matches!(
            self,
            NodeKind::Class | NodeKind::Interface | NodeKind::Struct | NodeKind::Trait
        )
    }

    /// Kinds that can appear as the source of a `CALLS` edge.
    pub fn is_callable(self) -> bool {
        matches!(self, NodeKind::Function | NodeKind::Method)
    }

    /// Map a TOK `SymbolKind` name back to a graph node kind, so the regex
    /// fallback and the tree-sitter extractor agree on vocabulary.
    pub fn from_symbol_kind(raw: &str) -> Option<Self> {
        Some(match raw {
            "Function" => NodeKind::Function,
            "Method" => NodeKind::Method,
            "Class" => NodeKind::Class,
            "Interface" => NodeKind::Interface,
            "Struct" => NodeKind::Struct,
            "Enum" => NodeKind::Enum,
            "Trait" => NodeKind::Trait,
            "Type" => NodeKind::Type,
            "Const" | "Constant" => NodeKind::Constant,
            "Variable" => NodeKind::Variable,
            "Module" => NodeKind::Module,
            "Import" => NodeKind::Import,
            "File" => NodeKind::File,
            _ => return None,
        })
    }

    /// The TOK `SymbolKind` name to project into the `symbols` table. The
    /// existing schema has no `Variable`, so those collapse to `Const`, and
    /// `File` has no symbol equivalent at all.
    pub fn to_symbol_kind(self) -> Option<&'static str> {
        Some(match self {
            NodeKind::Function => "Function",
            NodeKind::Method => "Method",
            NodeKind::Class => "Class",
            NodeKind::Interface => "Interface",
            NodeKind::Struct => "Struct",
            NodeKind::Enum => "Enum",
            NodeKind::Trait => "Trait",
            NodeKind::Type => "Type",
            NodeKind::Constant | NodeKind::Variable => "Const",
            NodeKind::Module => "Module",
            NodeKind::Import => "Import",
            NodeKind::File => return None,
        })
    }
}

/// How two nodes relate. Direction is always source -> target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EdgeKind {
    /// A file or type lexically owns the target.
    Contains,
    /// A callable invokes another callable.
    Calls,
    /// A file imports a symbol from another file.
    Imports,
    /// A class/struct inherits from a base.
    Extends,
    /// A class/struct satisfies an interface or trait.
    Implements,
    /// A type is mentioned in a signature or annotation.
    TypeRef,
}

impl EdgeKind {
    /// Whether this edge carries dependency meaning for a graph walk or rank.
    ///
    /// Everything except `Contains`. A file contains every symbol defined in
    /// it, so walking containment makes each of those symbols a neighbour of
    /// all the others and lets the file itself act as a hub with an in-degree
    /// no genuine dependency could reach — the ranking then answers "which file
    /// is biggest" instead of "what does this question touch".
    ///
    /// Containment is not useless, it is just structural: it is what builds the
    /// skeleton outline and the parent links in rendered output, neither of
    /// which is a walk.
    pub fn is_dependency(self) -> bool {
        !matches!(self, EdgeKind::Contains)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            EdgeKind::Contains => "contains",
            EdgeKind::Calls => "calls",
            EdgeKind::Imports => "imports",
            EdgeKind::Extends => "extends",
            EdgeKind::Implements => "implements",
            EdgeKind::TypeRef => "typeref",
        }
    }

    /// The `edges.edge_type` value to project into SQLite. These strings are a
    /// frozen contract: existing `tok mem relations --query-type` queries match
    /// on them.
    ///
    /// `Extends` deliberately collapses to `IMPLEMENTS`. The existing
    /// `EdgeType` enum has no `Extends` variant, and `--query-type
    /// class_hierarchy` filters on `IMPLEMENTS` alone, so emitting a literal
    /// `EXTENDS` would make inheritance edges invisible to a command that
    /// reports them today. The graph keeps the two distinct; only the
    /// projection merges them.
    pub fn to_sqlite_edge_type(self) -> &'static str {
        match self {
            EdgeKind::Contains => "CONTAINS",
            EdgeKind::Calls => "CALLS",
            EdgeKind::Imports => "IMPORTS",
            EdgeKind::Extends | EdgeKind::Implements => "IMPLEMENTS",
            EdgeKind::TypeRef => "TYPE_REF",
        }
    }
}

/// A half-open line range, 1-based and inclusive on both ends.
///
/// Inclusive because every consumer is human-facing (`file.rs:12-40`) or feeds
/// a slice of source lines; converting once here beats converting at each use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(start: u32, end: u32) -> Self {
        // A zero-width or inverted span would silently produce empty slices
        // downstream, so clamp rather than propagate nonsense.
        Self {
            start: start.max(1),
            end: end.max(start.max(1)),
        }
    }

    pub fn line_count(&self) -> u32 {
        self.end.saturating_sub(self.start) + 1
    }
}

/// One symbol in the graph.
///
/// `id` is graft-style and readable: `path/to/file.ts::SymbolName`, with a
/// `~2`, `~3`, ... ordinal appended when a file declares the same name more
/// than once. Readability matters because these ids surface in agent-facing
/// output and in the markdown layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeV1 {
    pub id: String,
    pub kind: NodeKind,
    pub name: String,
    /// Repo-relative, forward-slashed.
    pub file: String,
    pub span: Span,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    /// The enclosing type for a method, used to resolve typed receivers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exported: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,

    /// LLM-generated one-liner from `--deep`. Absent unless enrichment ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crux: Option<String>,
}

impl NodeV1 {
    pub fn new(id: String, kind: NodeKind, name: String, file: String, span: Span) -> Self {
        Self {
            id,
            kind,
            name,
            file,
            span,
            signature: None,
            doc: None,
            parent: None,
            exported: false,
            language: None,
            crux: None,
        }
    }

    /// `src/cache.ts:12-40`, the form used in agent-facing output.
    pub fn location(&self) -> String {
        format!("{}:{}-{}", self.file, self.span.start, self.span.end)
    }
}

/// A directed relationship between two node ids.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EdgeV1 {
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
}

impl EdgeV1 {
    pub fn new(from: impl Into<String>, to: impl Into<String>, kind: EdgeKind) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            kind,
        }
    }
}

/// Per-file bookkeeping, used by the drift probe to decide what to re-extract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntryV1 {
    pub path: String,
    /// Hex SHA-256 of the file contents at extraction time.
    pub hash: String,
    pub size: u64,
    pub language: String,
    pub node_count: u32,
}

/// The complete serialized graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphV1 {
    pub version: u32,
    /// Identifies which extractor produced this. Any change to extraction logic
    /// bumps this, which invalidates caches without needing a format bump.
    pub extractor: String,
    pub repo_id: String,
    pub nodes: Vec<NodeV1>,
    pub edges: Vec<EdgeV1>,
    pub files: Vec<FileEntryV1>,

    /// Sub-projects discovered in the layout. Defaulted rather than required so
    /// a graph written before scopes existed still loads.
    #[serde(default)]
    pub scopes: Vec<crate::graph::scopes::ScopeV1>,
}

impl GraphV1 {
    pub fn new(repo_id: impl Into<String>, extractor: impl Into<String>) -> Self {
        Self {
            version: GRAPH_FORMAT_VERSION,
            extractor: extractor.into(),
            repo_id: repo_id.into(),
            nodes: Vec::new(),
            edges: Vec::new(),
            files: Vec::new(),
            scopes: Vec::new(),
        }
    }

    /// The scopes to rank over. An older graph, or one from a plain repo, has
    /// none stored, and a single root scope is the correct reading of that.
    pub fn scopes(&self) -> Vec<crate::graph::scopes::ScopeV1> {
        if self.scopes.is_empty() {
            return crate::graph::scopes::root_only();
        }
        self.scopes.clone()
    }

    /// Whether retrieval should take the scope-aware path at all.
    pub fn is_monorepo(&self) -> bool {
        self.scopes.iter().filter(|s| !s.is_root()).count() > 0
    }

    /// Sort and deduplicate so serialization is byte-stable.
    ///
    /// Extraction order follows directory traversal, which varies by filesystem,
    /// so this must run before any write or hash.
    pub fn normalize(&mut self) {
        self.nodes.sort_by(|a, b| a.id.cmp(&b.id));
        self.nodes.dedup_by(|a, b| a.id == b.id);
        self.edges.sort();
        self.edges.dedup();
        self.files.sort_by(|a, b| a.path.cmp(&b.path));
        self.files.dedup_by(|a, b| a.path == b.path);
    }

    /// Index nodes by id for O(1) lookup during resolution and traversal.
    pub fn node_index(&self) -> BTreeMap<&str, &NodeV1> {
        self.nodes.iter().map(|n| (n.id.as_str(), n)).collect()
    }

    pub fn node(&self, id: &str) -> Option<&NodeV1> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// Whether this graph was written by a build we can still read.
    pub fn is_compatible(&self) -> bool {
        self.version == GRAPH_FORMAT_VERSION
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str) -> NodeV1 {
        NodeV1::new(
            id.to_string(),
            NodeKind::Function,
            "f".to_string(),
            "a.rs".to_string(),
            Span::new(1, 2),
        )
    }

    #[test]
    fn normalize_is_order_independent() {
        let mut a = GraphV1::new("r", "x");
        a.nodes = vec![node("b"), node("a"), node("c")];
        a.edges = vec![
            EdgeV1::new("b", "c", EdgeKind::Calls),
            EdgeV1::new("a", "b", EdgeKind::Calls),
        ];

        let mut b = GraphV1::new("r", "x");
        b.nodes = vec![node("c"), node("b"), node("a")];
        b.edges = vec![
            EdgeV1::new("a", "b", EdgeKind::Calls),
            EdgeV1::new("b", "c", EdgeKind::Calls),
        ];

        a.normalize();
        b.normalize();
        assert_eq!(a, b);
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
    }

    #[test]
    fn normalize_drops_duplicates() {
        let mut g = GraphV1::new("r", "x");
        g.nodes = vec![node("a"), node("a")];
        g.edges = vec![
            EdgeV1::new("a", "b", EdgeKind::Calls),
            EdgeV1::new("a", "b", EdgeKind::Calls),
        ];
        g.normalize();
        assert_eq!(g.nodes.len(), 1);
        assert_eq!(g.edges.len(), 1);
    }

    #[test]
    fn span_clamps_inverted_input() {
        let s = Span::new(10, 3);
        assert_eq!(s.start, 10);
        assert_eq!(s.end, 10);
        assert_eq!(s.line_count(), 1);
    }

    #[test]
    fn span_rejects_zero_lines() {
        // Tree-sitter reports 0-based rows; an off-by-one at the top of a file
        // must not produce line 0, which no editor can navigate to.
        let s = Span::new(0, 0);
        assert_eq!(s.start, 1);
        assert_eq!(s.end, 1);
    }

    #[test]
    fn optional_fields_stay_out_of_json() {
        let n = node("a");
        let json = serde_json::to_string(&n).unwrap();
        assert!(!json.contains("signature"), "got {json}");
        assert!(!json.contains("crux"), "got {json}");
        assert!(!json.contains("exported"), "got {json}");
    }

    #[test]
    fn unknown_fields_are_ignored_by_readers() {
        // Forward compatibility: an older binary must not choke on a graph
        // written by a newer one that added node metadata.
        let json = r#"{
            "id":"a","kind":"function","name":"f","file":"a.rs",
            "span":{"start":1,"end":2},"someFutureField":123
        }"#;
        let n: NodeV1 = serde_json::from_str(json).expect("forward compatible");
        assert_eq!(n.name, "f");
    }

    #[test]
    fn kind_mapping_round_trips_through_sqlite_vocabulary() {
        for kind in [
            NodeKind::Function,
            NodeKind::Method,
            NodeKind::Class,
            NodeKind::Interface,
            NodeKind::Struct,
            NodeKind::Enum,
            NodeKind::Trait,
            NodeKind::Type,
            NodeKind::Module,
            NodeKind::Import,
        ] {
            let sql = kind.to_symbol_kind().expect("has sqlite equivalent");
            assert_eq!(NodeKind::from_symbol_kind(sql), Some(kind), "{sql}");
        }
    }

    #[test]
    fn file_nodes_have_no_sqlite_equivalent() {
        assert_eq!(NodeKind::File.to_symbol_kind(), None);
    }

    #[test]
    fn sqlite_edge_names_match_existing_schema() {
        // These strings are what `tok mem relations` already queries on.
        assert_eq!(EdgeKind::Calls.to_sqlite_edge_type(), "CALLS");
        assert_eq!(EdgeKind::Imports.to_sqlite_edge_type(), "IMPORTS");
        assert_eq!(EdgeKind::Implements.to_sqlite_edge_type(), "IMPLEMENTS");
        assert_eq!(EdgeKind::TypeRef.to_sqlite_edge_type(), "TYPE_REF");
        assert_eq!(EdgeKind::Contains.to_sqlite_edge_type(), "CONTAINS");
    }

    #[test]
    fn extends_projects_as_implements_for_class_hierarchy() {
        // `--query-type class_hierarchy` filters on IMPLEMENTS only. If Extends
        // projected to a literal "EXTENDS", inheritance edges would vanish from
        // a command that reports them today.
        assert_eq!(EdgeKind::Extends.to_sqlite_edge_type(), "IMPLEMENTS");
        assert_ne!(EdgeKind::Extends, EdgeKind::Implements, "distinct in graph");
    }

    #[test]
    fn every_projected_edge_type_is_a_known_symbol_edge() {
        use crate::mem::symbols::EdgeType;
        for kind in [
            EdgeKind::Contains,
            EdgeKind::Calls,
            EdgeKind::Imports,
            EdgeKind::Extends,
            EdgeKind::Implements,
            EdgeKind::TypeRef,
        ] {
            let projected = kind.to_sqlite_edge_type();
            assert!(
                EdgeType::from_str(projected).is_some(),
                "{projected} is not a value the existing schema understands"
            );
        }
    }
}
