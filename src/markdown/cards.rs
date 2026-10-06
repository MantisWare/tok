//! Per-file wiring cards: what a file declares, and how it connects.
//!
//! A card answers the questions an agent asks before touching a file — what
//! lives here, what does it depend on, and what breaks if I change it — in a
//! form that is cheap to read and stays useful when checked into the repo.
//!
//! The content is derived entirely from the graph, so a card is reproducible
//! from source. What is *not* derived is the Notes section a human writes; see
//! [`crate::markdown::blocks`] for how that survives regeneration.

use std::collections::{BTreeMap, BTreeSet};

use crate::graph::types::{EdgeKind, EdgeV1, GraphV1, NodeKind, NodeV1};
use crate::markdown::frontmatter::Frontmatter;
use crate::markdown::slug;

/// A rendered card, ready to be merged into whatever is on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    /// Filename within the cards directory, including the `.md` extension.
    pub filename: String,
    /// The source file this card describes.
    pub path: String,
    /// Rendered YAML frontmatter, written above the generated block so that
    /// editors and note tools can parse it.
    pub frontmatter: String,
    /// Generated content, excluding frontmatter and the Notes section.
    pub body: String,
}

/// Build a card for every indexed file.
pub fn build_all(graph: &GraphV1) -> Vec<Card> {
    let paths: Vec<&str> = graph.files.iter().map(|f| f.path.as_str()).collect();
    let slugs = slug::unique_slugs(paths.iter().copied());
    let relations = Relations::build(graph);

    let mut cards: Vec<Card> = paths
        .iter()
        .map(|path| build_one(graph, &relations, path, &slugs[path]))
        .collect();

    cards.sort_by(|a, b| a.path.cmp(&b.path));
    cards
}

fn build_one(graph: &GraphV1, relations: &Relations, path: &str, slug: &str) -> Card {
    let symbols = symbols_in(graph, path);

    let mut frontmatter = Frontmatter::new();
    frontmatter
        .set("tok_kind", "file-card")
        .set("path", path)
        .set("symbols", symbols.len().to_string());

    if let Some(entry) = graph.files.iter().find(|f| f.path == path) {
        frontmatter.set("language", entry.language.clone());
    }

    let mut body = format!("# {path}\n");
    body.push_str(&render_symbols(&symbols));
    body.push_str(&render_dependencies(relations, path));
    body.push_str(&render_dependents(relations, path));

    Card {
        filename: format!("{slug}.md"),
        path: path.to_string(),
        frontmatter: frontmatter.render(),
        body: body.trim_end().to_string(),
    }
}

fn render_symbols(symbols: &[&NodeV1]) -> String {
    if symbols.is_empty() {
        return "\nNo symbols extracted from this file.\n".to_string();
    }

    let mut out = String::from("\n## Declares\n\n");

    for symbol in symbols {
        let signature = symbol
            .signature
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(|s| format!(" — `{}`", one_line(s)))
            .unwrap_or_default();

        out.push_str(&format!(
            "- **{}** ({}, line {}){}\n",
            symbol.name,
            symbol.kind.as_str(),
            symbol.span.start,
            signature
        ));
    }

    out
}

fn render_dependencies(relations: &Relations, path: &str) -> String {
    let Some(targets) = relations.outgoing.get(path) else {
        return String::new();
    };
    if targets.is_empty() {
        return String::new();
    }

    let mut out = String::from("\n## Depends on\n\n");
    for target in targets {
        out.push_str(&format!("- `{target}`\n"));
    }
    out
}

fn render_dependents(relations: &Relations, path: &str) -> String {
    let Some(sources) = relations.incoming.get(path) else {
        return String::new();
    };
    if sources.is_empty() {
        return String::new();
    }

    let mut out = String::from("\n## Used by\n\n");
    for source in sources {
        out.push_str(&format!("- `{source}`\n"));
    }
    out
}

/// File-to-file edges, collapsed from the symbol-level graph.
///
/// Cards are per file, so symbol edges have to be lifted to their files.
/// Self-edges are dropped: "src/a.ts depends on src/a.ts" is true of nearly
/// every file and tells the reader nothing.
pub struct Relations<'a> {
    pub outgoing: BTreeMap<&'a str, BTreeSet<&'a str>>,
    pub incoming: BTreeMap<&'a str, BTreeSet<&'a str>>,
}

impl<'a> Relations<'a> {
    pub fn build(graph: &'a GraphV1) -> Self {
        let owner: BTreeMap<&str, &str> = graph
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), n.file.as_str()))
            .collect();

        let mut outgoing: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        let mut incoming: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();

        for edge in graph.edges.iter().filter(|e| carries_dependency(e)) {
            let (Some(from), Some(to)) =
                (owner.get(edge.from.as_str()), owner.get(edge.to.as_str()))
            else {
                continue;
            };

            if from == to {
                continue;
            }

            outgoing.entry(from).or_default().insert(to);
            incoming.entry(to).or_default().insert(from);
        }

        Self { outgoing, incoming }
    }
}

/// Whether an edge represents a real dependency between files.
///
/// Containment is excluded because it is an artifact of where a symbol was
/// written, not a statement about coupling.
fn carries_dependency(edge: &EdgeV1) -> bool {
    edge.kind != EdgeKind::Contains
}

fn symbols_in<'a>(graph: &'a GraphV1, path: &str) -> Vec<&'a NodeV1> {
    let mut symbols: Vec<&NodeV1> = graph
        .nodes
        .iter()
        .filter(|n| n.file == path)
        .filter(|n| n.kind != NodeKind::File && n.kind != NodeKind::Import)
        .collect();

    symbols.sort_by_key(|n| (n.span.start, n.name.clone()));
    symbols
}

/// Collapse a multi-line signature so it cannot break the list item it sits in.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::{FileEntryV1, Span};

    fn node(id: &str, kind: NodeKind, name: &str, file: &str, start: u32) -> NodeV1 {
        NodeV1::new(
            id.to_string(),
            kind,
            name.to_string(),
            file.to_string(),
            Span::new(start, start + 4),
        )
    }

    fn file(path: &str) -> FileEntryV1 {
        FileEntryV1 {
            path: path.to_string(),
            hash: "h".to_string(),
            size: 1,
            language: "typescript".to_string(),
            node_count: 1,
        }
    }

    fn graph(nodes: Vec<NodeV1>, edges: Vec<EdgeV1>, files: Vec<FileEntryV1>) -> GraphV1 {
        let mut g = GraphV1::new("repo", "test");
        g.nodes = nodes;
        g.edges = edges;
        g.files = files;
        g.normalize();
        g
    }

    #[test]
    fn a_card_is_produced_for_every_indexed_file() {
        let g = graph(
            Vec::new(),
            Vec::new(),
            vec![file("src/a.ts"), file("src/b.ts")],
        );

        let cards = build_all(&g);

        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].path, "src/a.ts");
    }

    #[test]
    fn a_card_lists_what_its_file_declares() {
        let g = graph(
            vec![node("a", NodeKind::Class, "Cache", "src/a.ts", 10)],
            Vec::new(),
            vec![file("src/a.ts")],
        );

        let body = &build_all(&g)[0].body;

        assert!(body.contains("## Declares"));
        assert!(body.contains("**Cache**"));
        assert!(body.contains("class"));
        assert!(body.contains("line 10"));
    }

    /// Frontmatter is kept out of the body so the writer can place it at the
    /// very top of the file, which is the only place YAML frontmatter parses.
    #[test]
    fn frontmatter_records_the_path_and_symbol_count() {
        let g = graph(
            vec![node("a", NodeKind::Class, "Cache", "src/a.ts", 1)],
            Vec::new(),
            vec![file("src/a.ts")],
        );

        let card = &build_all(&g)[0];

        assert!(card.frontmatter.starts_with("---\n"));
        assert!(card.frontmatter.contains("path: src/a.ts"));
        assert!(card.frontmatter.contains(r#"symbols: "1""#));
        assert!(!card.body.contains("---"));
        assert!(card.body.starts_with("# src/a.ts"));
    }

    #[test]
    fn cross_file_edges_become_dependency_sections() {
        let g = graph(
            vec![
                node("a", NodeKind::Function, "caller", "src/a.ts", 1),
                node("b", NodeKind::Function, "callee", "src/b.ts", 1),
            ],
            vec![EdgeV1::new("a", "b", EdgeKind::Calls)],
            vec![file("src/a.ts"), file("src/b.ts")],
        );

        let cards = build_all(&g);
        let a = cards.iter().find(|c| c.path == "src/a.ts").expect("card a");
        let b = cards.iter().find(|c| c.path == "src/b.ts").expect("card b");

        assert!(a.body.contains("## Depends on"));
        assert!(a.body.contains("src/b.ts"));
        assert!(b.body.contains("## Used by"));
        assert!(b.body.contains("src/a.ts"));
    }

    /// Nearly every file's symbols reference each other, so listing the file as
    /// its own dependency would add a useless section to every card.
    #[test]
    fn a_file_is_never_listed_as_its_own_dependency() {
        let g = graph(
            vec![
                node("a", NodeKind::Function, "one", "src/a.ts", 1),
                node("b", NodeKind::Function, "two", "src/a.ts", 5),
            ],
            vec![EdgeV1::new("a", "b", EdgeKind::Calls)],
            vec![file("src/a.ts")],
        );

        let body = &build_all(&g)[0].body;

        assert!(!body.contains("## Depends on"));
    }

    #[test]
    fn containment_is_not_a_dependency() {
        let g = graph(
            vec![
                node("c", NodeKind::Class, "Cache", "src/a.ts", 1),
                node("m", NodeKind::Method, "get", "src/b.ts", 1),
            ],
            vec![EdgeV1::new("c", "m", EdgeKind::Contains)],
            vec![file("src/a.ts"), file("src/b.ts")],
        );

        let cards = build_all(&g);

        assert!(cards.iter().all(|c| !c.body.contains("## Depends on")));
    }

    #[test]
    fn a_file_with_no_symbols_says_so() {
        let g = graph(Vec::new(), Vec::new(), vec![file("src/empty.ts")]);

        assert!(build_all(&g)[0].body.contains("No symbols extracted"));
    }

    #[test]
    fn import_and_file_nodes_are_not_listed_as_declarations() {
        let g = graph(
            vec![
                node("f", NodeKind::File, "a.ts", "src/a.ts", 1),
                node("i", NodeKind::Import, "lodash", "src/a.ts", 1),
                node("fn", NodeKind::Function, "run", "src/a.ts", 3),
            ],
            Vec::new(),
            vec![file("src/a.ts")],
        );

        let card = &build_all(&g)[0];

        assert!(card.body.contains("**run**"));
        assert!(!card.body.contains("**lodash**"));
        assert!(card.frontmatter.contains(r#"symbols: "1""#));
    }

    /// A signature spanning lines would break the markdown list it sits in.
    #[test]
    fn multiline_signatures_are_collapsed() {
        let mut n = node("a", NodeKind::Function, "run", "src/a.ts", 1);
        n.signature = Some("function run(\n  x: number,\n): void".to_string());
        let g = graph(vec![n], Vec::new(), vec![file("src/a.ts")]);

        let body = &build_all(&g)[0].body;

        assert!(body.contains("function run( x: number, ): void"));
        assert_eq!(body.matches("- **run**").count(), 1);
    }

    #[test]
    fn filenames_are_derived_from_the_path() {
        let g = graph(Vec::new(), Vec::new(), vec![file("src/graph/cache.ts")]);

        assert_eq!(build_all(&g)[0].filename, "src-graph-cache-ts.md");
    }

    #[test]
    fn card_generation_is_deterministic() {
        let g = graph(
            vec![
                node("a", NodeKind::Function, "one", "src/a.ts", 1),
                node("b", NodeKind::Function, "two", "src/b.ts", 1),
            ],
            vec![EdgeV1::new("a", "b", EdgeKind::Calls)],
            vec![file("src/a.ts"), file("src/b.ts")],
        );

        let first = build_all(&g);
        for _ in 0..5 {
            assert_eq!(build_all(&g), first);
        }
    }

    #[test]
    fn an_empty_graph_produces_no_cards() {
        assert!(build_all(&graph(Vec::new(), Vec::new(), Vec::new())).is_empty());
    }
}
