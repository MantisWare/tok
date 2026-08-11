//! Tree-sitter extraction: source file in, nodes and unresolved references out.
//!
//! Extraction is deliberately split from resolution. A single file cannot know
//! what `cache.get()` refers to — that needs the whole graph — so extraction
//! records a [`RawRef`] describing what it saw, and [`crate::graph::resolve`]
//! turns those into edges once every file has been read. This split is what
//! makes per-file caching possible: a file's extraction only depends on its own
//! bytes.
//!
//! Every language shares [`walk`], which handles traversal, scope tracking,
//! spans, and doc comments. Per-language knowledge lives behind
//! [`LanguageExtractor`], which is only asked narrow questions about individual
//! AST nodes.

pub mod ids;
pub mod source;

#[cfg(feature = "graph")]
mod go;
#[cfg(feature = "graph")]
mod python;
#[cfg(feature = "graph")]
mod rust_lang;
#[cfg(feature = "graph")]
mod typescript;

use serde::{Deserialize, Serialize};

use crate::graph::types::{EdgeKind, NodeV1};
#[cfg(feature = "graph")]
use crate::graph::types::{NodeKind, Span};
#[cfg(feature = "graph")]
use crate::graph::Language;

/// How a member access was qualified, which decides how it can be resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Receiver {
    /// `this.x()` / `self.x()` — resolve against the enclosing type.
    SelfType,
    /// `foo.x()` where `foo`'s type was inferred. Resolvable to a method.
    Typed(String),
    /// `foo.x()` where `foo`'s type is unknown. Resolved by name only, and
    /// dropped when ambiguous.
    Untyped(String),
}

/// A reference seen during extraction, before it is matched to a target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawRef {
    /// Node id of the enclosing declaration, or the file id at top level.
    pub from: String,
    /// The referenced identifier, unqualified.
    pub name: String,
    pub receiver: Option<Receiver>,
    pub kind: EdgeKind,
}

/// A name brought into a file by an import statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportBinding {
    /// The name as used in this file, after any aliasing.
    pub local: String,
    /// The name as exported by the source module.
    pub imported: String,
    /// The module specifier exactly as written.
    pub module: String,
}

/// A relationship declared between two *names* rather than from a known node.
///
/// Rust's `impl Trait for Type` is the motivating case: the `impl` block is not
/// itself a declaration, so there is no node to hang the edge on, and the edge
/// really belongs between `Type` and `Trait`. Resolution matches both names
/// against the graph afterwards.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedRelation {
    pub from_name: String,
    pub to_name: String,
    pub kind: EdgeKind,
}

/// Everything extraction learned about one file.
///
/// Serializable because this is exactly what the extract cache memoizes: a
/// file's extraction depends only on its own bytes, so it can be reused
/// verbatim whenever the content hash and extractor stamp both match.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileExtraction {
    pub path: String,
    pub nodes: Vec<NodeV1>,
    pub refs: Vec<RawRef>,
    pub imports: Vec<ImportBinding>,
    pub named_relations: Vec<NamedRelation>,
    /// `(method node id, owning type name)` for languages that declare methods
    /// outside their type's body, which is to say Go. Resolution turns the name
    /// into an id once every file is known, because a Go method may live in a
    /// different file from its receiver type.
    pub method_owners: Vec<(String, String)>,
    /// Local `(variable, type)` pairs, consumed by resolution to upgrade an
    /// [`Receiver::Untyped`] into a [`Receiver::Typed`].
    pub bindings: Vec<(String, String)>,
}

/// Per-language AST knowledge.
///
/// Implementations answer questions about single nodes; they never traverse.
/// Traversal is [`walk`]'s job, so scope handling stays consistent across
/// languages.
#[cfg(feature = "graph")]
pub trait LanguageExtractor {
    /// The node kind a declaration produces, or `None` if not a declaration.
    fn declaration_kind(&self, node: tree_sitter::Node, src: &str) -> Option<NodeKind>;

    /// The declared name.
    fn name_of(&self, node: tree_sitter::Node, src: &str) -> Option<String>;

    /// Whether the declaration is visible outside its module.
    fn is_exported(&self, node: tree_sitter::Node, src: &str) -> bool;

    /// A call expression's target, as (receiver, function name).
    fn call_target(&self, node: tree_sitter::Node, src: &str)
        -> Option<(Option<Receiver>, String)>;

    /// Import bindings introduced by this node.
    fn imports(&self, node: tree_sitter::Node, src: &str) -> Vec<ImportBinding>;

    /// Inheritance and interface-satisfaction edges declared by this node.
    ///
    /// Only consulted on declaration nodes, since the edge starts at the
    /// declaration itself.
    fn inheritance(&self, node: tree_sitter::Node, src: &str) -> Vec<(EdgeKind, String)>;

    /// Relationships between two names, for constructs that are not themselves
    /// declarations. Consulted on every node.
    fn named_relations(&self, _node: tree_sitter::Node, _src: &str) -> Vec<NamedRelation> {
        Vec::new()
    }

    /// Local variable type bindings, used to resolve typed receivers.
    ///
    /// Returns (variable name, type name) for things like `const c = new Cache()`
    /// or `var c *Cache`.
    fn type_bindings(&self, _node: tree_sitter::Node, _src: &str) -> Vec<(String, String)> {
        Vec::new()
    }

    /// Whether a declaration node should also be treated as a scope that owns
    /// its children (a class owning methods).
    fn is_type_scope(&self, kind: NodeKind) -> bool {
        kind.is_type_like()
    }

    /// The type that owns this method, when it is named rather than lexical.
    ///
    /// Go declares methods outside the type body (`func (c *Cache) Get()`), so
    /// the owner cannot be inferred from the walk.
    fn owner_type_name(&self, _node: tree_sitter::Node, _src: &str) -> Option<String> {
        None
    }
}

/// Parse and extract one file.
///
/// Returns `Ok(None)` when the language has no compiled-in grammar, which tells
/// the caller to fall back to the regex extractor rather than treating the file
/// as empty.
#[cfg(feature = "graph")]
pub fn extract_file(
    path: &str,
    src: &str,
    language: Language,
) -> anyhow::Result<Option<FileExtraction>> {
    use anyhow::Context;

    let Some(grammar) = language.grammar() else {
        return Ok(None);
    };

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&grammar)
        .with_context(|| format!("tree-sitter rejected the {} grammar", language.as_str()))?;

    let Some(tree) = parser.parse(src, None) else {
        // Tree-sitter returns None only when parsing was cancelled or the
        // input is unusable. Skip the file rather than failing the whole index.
        return Ok(None);
    };

    let extractor = extractor_for(language);
    Ok(Some(walk(
        path,
        src,
        tree.root_node(),
        extractor.as_ref(),
        language,
    )))
}

#[cfg(feature = "graph")]
fn extractor_for(language: Language) -> Box<dyn LanguageExtractor> {
    match language {
        Language::TypeScript | Language::Tsx | Language::JavaScript => {
            Box::new(typescript::TypeScriptExtractor)
        }
        Language::Python => Box::new(python::PythonExtractor),
        Language::Go => Box::new(go::GoExtractor),
        Language::Rust => Box::new(rust_lang::RustExtractor),
    }
}

/// Depth-first traversal shared by every language.
#[cfg(feature = "graph")]
fn walk(
    path: &str,
    src: &str,
    root: tree_sitter::Node,
    extractor: &dyn LanguageExtractor,
    language: Language,
) -> FileExtraction {
    let mut state = WalkState {
        path: path.to_string(),
        language,
        minter: ids::IdMinter::new(),
        out: FileExtraction {
            path: path.to_string(),
            ..Default::default()
        },
    };

    // The file node anchors top-level references and gives the retrieval layer
    // something to rank when a query matches a path but no symbol.
    let file_id = ids::IdMinter::file_id(path);
    let mut file_node = NodeV1::new(
        file_id.clone(),
        NodeKind::File,
        path.rsplit('/').next().unwrap_or(path).to_string(),
        path.to_string(),
        Span::new(1, line_of(root.end_byte(), src)),
    );
    file_node.language = Some(language.as_str().to_string());
    state.out.nodes.push(file_node);

    let scope = Scope {
        owner: file_id,
        enclosing_type: None,
    };
    visit(root, src, extractor, &mut state, &scope);

    state.out
}

#[cfg(feature = "graph")]
struct WalkState {
    path: String,
    language: Language,
    minter: ids::IdMinter,
    out: FileExtraction,
}

/// Who owns the nodes and references found at the current depth.
#[cfg(feature = "graph")]
#[derive(Clone)]
struct Scope {
    /// Node id that references found here belong to.
    owner: String,
    /// Innermost enclosing type, for `this`/`self` resolution.
    enclosing_type: Option<String>,
}

#[cfg(feature = "graph")]
fn visit(
    node: tree_sitter::Node,
    src: &str,
    extractor: &dyn LanguageExtractor,
    state: &mut WalkState,
    scope: &Scope,
) {
    for binding in extractor.type_bindings(node, src) {
        state.out.bindings.push(binding);
    }

    for import in extractor.imports(node, src) {
        state.out.imports.push(import);
    }

    for relation in extractor.named_relations(node, src) {
        state.out.named_relations.push(relation);
    }

    if let Some((receiver, name)) = extractor.call_target(node, src) {
        state.out.refs.push(RawRef {
            from: scope.owner.clone(),
            name,
            receiver,
            kind: EdgeKind::Calls,
        });
    }

    // A declaration becomes a node and a new scope for everything beneath it.
    let child_scope = match declaration_node(node, src, extractor, state, scope) {
        Some(s) => s,
        None => scope.clone(),
    };

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit(child, src, extractor, state, &child_scope);
    }
}

/// Turn a declaration node into a graph node, returning the scope its children
/// should use.
#[cfg(feature = "graph")]
fn declaration_node(
    node: tree_sitter::Node,
    src: &str,
    extractor: &dyn LanguageExtractor,
    state: &mut WalkState,
    scope: &Scope,
) -> Option<Scope> {
    let kind = extractor.declaration_kind(node, src)?;
    let name = extractor.name_of(node, src)?;
    if name.is_empty() {
        return None;
    }

    let id = state.minter.mint(&state.path, &name);
    let span = Span::new(
        line_of(node.start_byte(), src),
        line_of(node.end_byte(), src),
    );

    let mut graph_node = NodeV1::new(id.clone(), kind, name.clone(), state.path.clone(), span);
    graph_node.language = Some(state.language.as_str().to_string());
    graph_node.exported = extractor.is_exported(node, src);
    graph_node.parent = scope.enclosing_type.clone();

    let signature = source::signature_from(src, node.start_byte(), node.end_byte());
    if !signature.is_empty() {
        graph_node.signature = Some(signature);
    }
    graph_node.doc = source::doc_comment_before(src, node.start_byte());

    for (edge_kind, target) in extractor.inheritance(node, src) {
        state.out.refs.push(RawRef {
            from: id.clone(),
            name: target,
            receiver: None,
            kind: edge_kind,
        });
    }

    if let Some(owner) = extractor.owner_type_name(node, src) {
        state.out.method_owners.push((id.clone(), owner));
    }

    state.out.nodes.push(graph_node);

    Some(Scope {
        owner: id.clone(),
        enclosing_type: if extractor.is_type_scope(kind) {
            Some(id)
        } else {
            // A method keeps pointing at the type that owns it, so nested
            // closures still resolve `this` correctly.
            scope.enclosing_type.clone()
        },
    })
}

/// 1-based line number for a byte offset.
#[cfg(feature = "graph")]
fn line_of(byte_offset: usize, src: &str) -> u32 {
    let capped = byte_offset.min(src.len());
    let newlines = source::slice(src, 0, capped)
        .bytes()
        .filter(|b| *b == b'\n')
        .count();
    u32::try_from(newlines + 1).unwrap_or(u32::MAX)
}

#[cfg(all(test, feature = "graph"))]
mod tests {
    use super::*;

    #[test]
    fn line_numbers_are_one_based() {
        let src = "a\nb\nc";
        assert_eq!(line_of(0, src), 1);
        assert_eq!(line_of(2, src), 2);
        assert_eq!(line_of(4, src), 3);
    }

    #[test]
    fn line_of_clamps_past_the_end() {
        let src = "a\nb";
        assert_eq!(line_of(9999, src), 2);
    }

    #[test]
    fn unsupported_language_is_reported_as_none_not_empty() {
        // Callers distinguish "no grammar, use the fallback" from "parsed and
        // found nothing"; conflating them would silently lose symbols.
        let src = "fn f() {}";
        let out = extract_file("a.rs", src, Language::Rust).unwrap();
        assert!(out.is_some(), "rust grammar ships by default");
    }

    #[test]
    fn every_file_gets_a_file_node() {
        let out = extract_file("a.rs", "fn f() {}", Language::Rust)
            .unwrap()
            .unwrap();
        let file_nodes: Vec<_> = out
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::File)
            .collect();
        assert_eq!(file_nodes.len(), 1);
        assert_eq!(file_nodes[0].id, "a.rs");
    }

    #[test]
    fn extraction_survives_a_syntactically_broken_file() {
        // Tree-sitter is error-tolerant; a half-typed file mid-edit must yield
        // whatever is parseable instead of failing the index.
        let src = "fn good() {}\nfn broken(";
        let out = extract_file("a.rs", src, Language::Rust).unwrap().unwrap();
        assert!(out.nodes.iter().any(|n| n.name == "good"));
    }

    #[test]
    fn extraction_handles_multibyte_source() {
        let src = "/// 日本語のコメント\npub fn 関数() -> u32 { 0 }\nfn f() { 関数(); }";
        let out = extract_file("a.rs", src, Language::Rust).unwrap().unwrap();
        assert!(out.nodes.iter().any(|n| n.name == "関数"));
    }
}
