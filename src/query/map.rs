//! Repository overview: the shape of a codebase in one screen.
//!
//! Answers the question an agent has on its first turn in an unfamiliar repo —
//! "what is this and where do I start" — which it otherwise answers by listing
//! directories and reading README files and a dozen entry points, at a cost of
//! thousands of tokens for information the graph already holds.
//!
//! Three signals, each chosen because it survives contact with real repos:
//!
//! - **Directory weight** shows where the code actually lives, which is often
//!   not where the directory names suggest.
//! - **Hubs** are the most depended-upon symbols. These are the things worth
//!   understanding before changing anything, because they have the most
//!   callers to break.
//! - **Entry points** are exported symbols nothing else in the repo calls,
//!   which is the practical definition of a public surface.

use std::collections::BTreeMap;

use crate::graph::types::{EdgeKind, GraphV1, NodeKind, NodeV1};

/// A directory and how much of the repo lives under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectorySummary {
    pub path: String,
    pub files: usize,
    pub symbols: usize,
    /// Most common language in this directory, for a mixed-language repo.
    pub language: String,
}

/// A heavily depended-upon symbol.
#[derive(Debug, Clone)]
pub struct Hub<'a> {
    pub node: &'a NodeV1,
    /// Number of distinct symbols referencing this one.
    pub dependents: usize,
}

#[derive(Debug, Clone)]
pub struct RepoMap<'a> {
    pub file_count: usize,
    pub symbol_count: usize,
    pub edge_count: usize,
    pub languages: Vec<(String, usize)>,
    pub directories: Vec<DirectorySummary>,
    pub hubs: Vec<Hub<'a>>,
    pub entry_points: Vec<&'a NodeV1>,
}

#[derive(Debug, Clone)]
pub struct MapOptions {
    pub max_directories: usize,
    pub max_hubs: usize,
    pub max_entry_points: usize,
}

impl Default for MapOptions {
    fn default() -> Self {
        Self {
            max_directories: 15,
            max_hubs: 15,
            max_entry_points: 10,
        }
    }
}

/// Build the overview.
pub fn build<'a>(graph: &'a GraphV1, options: &MapOptions) -> RepoMap<'a> {
    let symbols: Vec<&NodeV1> = graph
        .nodes
        .iter()
        .filter(|n| n.kind != NodeKind::File && n.kind != NodeKind::Import)
        .collect();

    RepoMap {
        file_count: graph.files.len(),
        symbol_count: symbols.len(),
        edge_count: graph.edges.len(),
        languages: languages(graph),
        directories: directories(graph, &symbols, options.max_directories),
        hubs: hubs(graph, &symbols, options.max_hubs),
        entry_points: entry_points(graph, &symbols, options.max_entry_points),
    }
}

fn languages(graph: &GraphV1) -> Vec<(String, usize)> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for file in &graph.files {
        *counts.entry(file.language.as_str()).or_insert(0) += 1;
    }

    let mut out: Vec<(String, usize)> = counts
        .into_iter()
        .map(|(name, count)| (name.to_string(), count))
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}

fn directories(graph: &GraphV1, symbols: &[&NodeV1], limit: usize) -> Vec<DirectorySummary> {
    let mut files: BTreeMap<&str, (usize, BTreeMap<&str, usize>)> = BTreeMap::new();

    for file in &graph.files {
        let entry = files.entry(parent_dir(&file.path)).or_default();
        entry.0 += 1;
        *entry.1.entry(file.language.as_str()).or_insert(0) += 1;
    }

    let mut symbol_counts: BTreeMap<&str, usize> = BTreeMap::new();
    for symbol in symbols {
        *symbol_counts.entry(parent_dir(&symbol.file)).or_insert(0) += 1;
    }

    let mut out: Vec<DirectorySummary> = files
        .into_iter()
        .map(|(path, (count, languages))| DirectorySummary {
            path: path.to_string(),
            files: count,
            symbols: symbol_counts.get(path).copied().unwrap_or(0),
            language: languages
                .into_iter()
                .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(a.0)))
                .map(|(name, _)| name.to_string())
                .unwrap_or_default(),
        })
        .collect();

    out.sort_by(|a, b| {
        b.symbols
            .cmp(&a.symbols)
            .then_with(|| b.files.cmp(&a.files))
            .then_with(|| a.path.cmp(&b.path))
    });
    out.truncate(limit);
    out
}

fn hubs<'a>(graph: &GraphV1, symbols: &[&'a NodeV1], limit: usize) -> Vec<Hub<'a>> {
    let mut dependents: BTreeMap<&str, usize> = BTreeMap::new();

    for edge in &graph.edges {
        // Containment is structural bookkeeping, not a dependency: every method
        // would otherwise make its class look like a hub.
        if edge.kind == EdgeKind::Contains {
            continue;
        }
        *dependents.entry(edge.to.as_str()).or_insert(0) += 1;
    }

    let mut out: Vec<Hub> = symbols
        .iter()
        .filter_map(|node| {
            let count = dependents.get(node.id.as_str()).copied().unwrap_or(0);
            (count > 0).then_some(Hub {
                node,
                dependents: count,
            })
        })
        .collect();

    out.sort_by(|a, b| {
        b.dependents
            .cmp(&a.dependents)
            .then_with(|| a.node.file.cmp(&b.node.file))
            .then_with(|| a.node.name.cmp(&b.node.name))
    });
    out.truncate(limit);
    out
}

fn entry_points<'a>(graph: &GraphV1, symbols: &[&'a NodeV1], limit: usize) -> Vec<&'a NodeV1> {
    let referenced: std::collections::BTreeSet<&str> = graph
        .edges
        .iter()
        .filter(|e| e.kind != EdgeKind::Contains)
        .map(|e| e.to.as_str())
        .collect();

    let mut out: Vec<&NodeV1> = symbols
        .iter()
        .filter(|node| node.exported)
        .filter(|node| !referenced.contains(node.id.as_str()))
        .copied()
        .collect();

    out.sort_by(|a, b| a.file.cmp(&b.file).then_with(|| a.name.cmp(&b.name)));
    out.truncate(limit);
    out
}

/// The directory containing a repo-relative path; `.` for files at the root.
fn parent_dir(path: &str) -> &str {
    match path.rfind('/') {
        Some(index) => &path[..index],
        None => ".",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::{EdgeV1, FileEntryV1, Span};

    fn node(id: &str, name: &str, file: &str, exported: bool) -> NodeV1 {
        let mut n = NodeV1::new(
            id.to_string(),
            NodeKind::Function,
            name.to_string(),
            file.to_string(),
            Span::new(1, 5),
        );
        n.exported = exported;
        n
    }

    fn file(path: &str, language: &str) -> FileEntryV1 {
        FileEntryV1 {
            path: path.to_string(),
            hash: "h".to_string(),
            size: 10,
            language: language.to_string(),
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
    fn counts_summarize_the_graph() {
        let g = graph(
            vec![node("a", "one", "src/a.ts", false)],
            vec![EdgeV1::new("a", "a", EdgeKind::Calls)],
            vec![file("src/a.ts", "typescript")],
        );

        let map = build(&g, &MapOptions::default());

        assert_eq!(map.file_count, 1);
        assert_eq!(map.symbol_count, 1);
        assert_eq!(map.edge_count, 1);
    }

    #[test]
    fn directories_are_ranked_by_symbol_weight() {
        let g = graph(
            vec![
                node("a", "one", "src/core/a.ts", false),
                node("b", "two", "src/core/b.ts", false),
                node("c", "three", "docs/c.ts", false),
            ],
            Vec::new(),
            vec![
                file("src/core/a.ts", "typescript"),
                file("src/core/b.ts", "typescript"),
                file("docs/c.ts", "typescript"),
            ],
        );

        let map = build(&g, &MapOptions::default());

        assert_eq!(map.directories[0].path, "src/core");
        assert_eq!(map.directories[0].symbols, 2);
    }

    #[test]
    fn a_root_level_file_lands_in_the_dot_directory() {
        let g = graph(
            vec![node("a", "one", "index.ts", false)],
            Vec::new(),
            vec![file("index.ts", "typescript")],
        );

        assert_eq!(build(&g, &MapOptions::default()).directories[0].path, ".");
    }

    #[test]
    fn hubs_are_the_most_depended_upon_symbols() {
        let g = graph(
            vec![
                node("hub", "shared", "src/a.ts", false),
                node("x", "x", "src/b.ts", false),
                node("y", "y", "src/c.ts", false),
            ],
            vec![
                EdgeV1::new("x", "hub", EdgeKind::Calls),
                EdgeV1::new("y", "hub", EdgeKind::Calls),
            ],
            vec![file("src/a.ts", "typescript")],
        );

        let map = build(&g, &MapOptions::default());

        assert_eq!(map.hubs[0].node.name, "shared");
        assert_eq!(map.hubs[0].dependents, 2);
    }

    /// Without this exclusion the biggest class in the repo is always the top
    /// hub, purely for having many methods.
    #[test]
    fn containment_does_not_make_a_symbol_a_hub() {
        let g = graph(
            vec![
                node("class", "Cache", "src/a.ts", false),
                node("m", "get", "src/a.ts", false),
            ],
            vec![EdgeV1::new("class", "m", EdgeKind::Contains)],
            vec![file("src/a.ts", "typescript")],
        );

        assert!(build(&g, &MapOptions::default()).hubs.is_empty());
    }

    #[test]
    fn entry_points_are_exported_and_uncalled() {
        let g = graph(
            vec![
                node("public", "main", "src/index.ts", true),
                node("internal", "helper", "src/util.ts", true),
                node("caller", "caller", "src/a.ts", false),
            ],
            vec![EdgeV1::new("caller", "internal", EdgeKind::Calls)],
            vec![file("src/index.ts", "typescript")],
        );

        let map = build(&g, &MapOptions::default());
        let names: Vec<&str> = map.entry_points.iter().map(|n| n.name.as_str()).collect();

        assert_eq!(names, vec!["main"]);
    }

    #[test]
    fn unexported_symbols_are_never_entry_points() {
        let g = graph(
            vec![node("a", "private", "src/a.ts", false)],
            Vec::new(),
            vec![file("src/a.ts", "typescript")],
        );

        assert!(build(&g, &MapOptions::default()).entry_points.is_empty());
    }

    #[test]
    fn languages_are_ranked_by_file_count() {
        let g = graph(
            Vec::new(),
            Vec::new(),
            vec![
                file("a.ts", "typescript"),
                file("b.ts", "typescript"),
                file("c.py", "python"),
            ],
        );

        let map = build(&g, &MapOptions::default());

        assert_eq!(map.languages[0], ("typescript".to_string(), 2));
        assert_eq!(map.languages[1], ("python".to_string(), 1));
    }

    #[test]
    fn limits_are_respected() {
        let nodes: Vec<NodeV1> = (0..30)
            .map(|i| {
                node(
                    &format!("n{i}"),
                    &format!("n{i}"),
                    &format!("d{i}/f.ts"),
                    true,
                )
            })
            .collect();
        let files: Vec<FileEntryV1> = (0..30)
            .map(|i| file(&format!("d{i}/f.ts"), "typescript"))
            .collect();
        let g = graph(nodes, Vec::new(), files);

        let options = MapOptions {
            max_directories: 3,
            max_hubs: 2,
            max_entry_points: 4,
        };
        let map = build(&g, &options);

        assert_eq!(map.directories.len(), 3);
        assert_eq!(map.entry_points.len(), 4);
    }

    #[test]
    fn an_empty_graph_maps_to_empty_sections() {
        let g = graph(Vec::new(), Vec::new(), Vec::new());
        let map = build(&g, &MapOptions::default());

        assert_eq!(map.file_count, 0);
        assert!(map.directories.is_empty());
        assert!(map.hubs.is_empty());
        assert!(map.entry_points.is_empty());
    }

    #[test]
    fn the_map_is_deterministic() {
        let g = graph(
            vec![
                node("a", "one", "src/a.ts", true),
                node("b", "two", "src/b.ts", true),
            ],
            Vec::new(),
            vec![
                file("src/a.ts", "typescript"),
                file("src/b.ts", "typescript"),
            ],
        );

        let first = build(&g, &MapOptions::default());
        for _ in 0..5 {
            let again = build(&g, &MapOptions::default());
            assert_eq!(again.directories, first.directories);
            assert_eq!(
                again.entry_points.iter().map(|n| &n.id).collect::<Vec<_>>(),
                first.entry_points.iter().map(|n| &n.id).collect::<Vec<_>>()
            );
        }
    }
}
