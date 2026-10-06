//! File outlines: signatures without bodies.
//!
//! This is the single highest-leverage token saving in the retrieval layer. An
//! agent that needs to know what a 2,000-line module offers currently reads all
//! 2,000 lines; the skeleton answers the same question in the forty lines that
//! actually declare something. Bodies are where the tokens are, and they are
//! almost never what the agent needed in order to decide *whether* to read the
//! file.
//!
//! The outline is built entirely from the graph, so it costs no file I/O and no
//! reparsing.

use crate::graph::types::{EdgeKind, GraphV1, NodeKind, NodeV1};

/// One line of an outline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry<'a> {
    pub node: &'a NodeV1,
    /// Nesting level: methods sit one level under their owning type.
    pub depth: usize,
}

/// Build the outline for a single file.
///
/// Entries come back in source order at each level, with members nested under
/// their owner, because an outline that jumps around the file is harder to read
/// than the file itself.
pub fn outline<'a>(graph: &'a GraphV1, file: &str) -> Vec<Entry<'a>> {
    let owners = ownership(graph);

    let mut roots: Vec<&NodeV1> = graph
        .nodes
        .iter()
        .filter(|node| node.file == file)
        .filter(|node| node.kind != NodeKind::File && node.kind != NodeKind::Import)
        .filter(|node| {
            // A member is emitted under its owner, not at the top level. An
            // owner in a *different* file (a Go method on a type declared
            // elsewhere) still surfaces here, or it would vanish from both
            // outlines.
            match owners.iter().find(|(child, _)| *child == node.id.as_str()) {
                Some((_, owner)) => !graph.nodes.iter().any(|n| n.id == *owner && n.file == file),
                None => true,
            }
        })
        .collect();

    roots.sort_by_key(|node| (node.span.start, node.span.end, node.name.clone()));

    let mut entries = Vec::new();
    for root in roots {
        entries.push(Entry {
            node: root,
            depth: 0,
        });

        let mut members: Vec<&NodeV1> = owners
            .iter()
            .filter(|(_, owner)| *owner == root.id.as_str())
            .filter_map(|(child, _)| graph.nodes.iter().find(|n| n.id == *child))
            .collect();
        members.sort_by_key(|node| (node.span.start, node.name.clone()));

        for member in members {
            entries.push(Entry {
                node: member,
                depth: 1,
            });
        }
    }

    entries
}

/// Every file the graph knows about, sorted.
pub fn files(graph: &GraphV1) -> Vec<&str> {
    let mut files: Vec<&str> = graph.files.iter().map(|f| f.path.as_str()).collect();
    files.sort_unstable();
    files.dedup();
    files
}

/// Child-to-owner pairs derived from `CONTAINS` edges between symbols.
///
/// File containment is skipped: every symbol is contained by its file, and
/// nesting the whole outline one level under the filename adds indentation
/// without adding information.
fn ownership(graph: &GraphV1) -> Vec<(&str, &str)> {
    graph
        .edges
        .iter()
        .filter(|edge| edge.kind == EdgeKind::Contains)
        .filter(|edge| {
            graph
                .nodes
                .iter()
                .any(|n| n.id == edge.from && n.kind != NodeKind::File)
        })
        .map(|edge| (edge.to.as_str(), edge.from.as_str()))
        .collect()
}

/// Render an outline as text.
///
/// Prefers the recorded signature and falls back to `kind name`, because a
/// signature is what tells the reader how to call the thing.
pub fn render(entries: &[Entry]) -> String {
    let mut out = String::new();

    for entry in entries {
        let indent = "  ".repeat(entry.depth);
        let body = match &entry.node.signature {
            Some(signature) if !signature.is_empty() => signature.clone(),
            _ => format!("{} {}", entry.node.kind.as_str(), entry.node.name),
        };

        out.push_str(&format!("{indent}{body}  ({})\n", entry.node.span.start));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::{EdgeV1, Span};

    fn node(id: &str, kind: NodeKind, name: &str, file: &str, start: u32) -> NodeV1 {
        NodeV1::new(
            id.to_string(),
            kind,
            name.to_string(),
            file.to_string(),
            Span::new(start, start + 5),
        )
    }

    fn graph(nodes: Vec<NodeV1>, edges: Vec<EdgeV1>) -> GraphV1 {
        let mut g = GraphV1::new("repo", "test");
        g.nodes = nodes;
        g.edges = edges;
        g.normalize();
        g
    }

    fn names<'a>(entries: &[Entry<'a>]) -> Vec<&'a str> {
        entries.iter().map(|e| e.node.name.as_str()).collect()
    }

    #[test]
    fn entries_follow_source_order() {
        let g = graph(
            vec![
                node("b", NodeKind::Function, "second", "src/a.ts", 20),
                node("a", NodeKind::Function, "first", "src/a.ts", 5),
            ],
            Vec::new(),
        );

        assert_eq!(names(&outline(&g, "src/a.ts")), vec!["first", "second"]);
    }

    #[test]
    fn methods_nest_under_their_class() {
        let g = graph(
            vec![
                node("c", NodeKind::Class, "Cache", "src/a.ts", 1),
                node("m", NodeKind::Method, "get", "src/a.ts", 5),
            ],
            vec![EdgeV1::new("c", "m", EdgeKind::Contains)],
        );

        let entries = outline(&g, "src/a.ts");

        assert_eq!(names(&entries), vec!["Cache", "get"]);
        assert_eq!(entries[0].depth, 0);
        assert_eq!(entries[1].depth, 1);
    }

    #[test]
    fn only_the_requested_file_appears() {
        let g = graph(
            vec![
                node("a", NodeKind::Function, "here", "src/a.ts", 1),
                node("b", NodeKind::Function, "elsewhere", "src/b.ts", 1),
            ],
            Vec::new(),
        );

        assert_eq!(names(&outline(&g, "src/a.ts")), vec!["here"]);
    }

    #[test]
    fn file_and_import_nodes_are_omitted() {
        let g = graph(
            vec![
                node("f", NodeKind::File, "a.ts", "src/a.ts", 1),
                node("i", NodeKind::Import, "lodash", "src/a.ts", 1),
                node("fn", NodeKind::Function, "run", "src/a.ts", 3),
            ],
            Vec::new(),
        );

        assert_eq!(names(&outline(&g, "src/a.ts")), vec!["run"]);
    }

    /// A Go method lives in a different file from its struct. It has to appear
    /// in its own file's outline, or reading that file's skeleton would show
    /// nothing at all.
    #[test]
    fn a_member_owned_from_another_file_still_appears_in_its_own() {
        let g = graph(
            vec![
                node("t", NodeKind::Struct, "Cache", "src/type.go", 1),
                node("m", NodeKind::Method, "Get", "src/methods.go", 3),
            ],
            vec![EdgeV1::new("t", "m", EdgeKind::Contains)],
        );

        assert_eq!(names(&outline(&g, "src/methods.go")), vec!["Get"]);
        assert_eq!(names(&outline(&g, "src/type.go")), vec!["Cache", "Get"]);
    }

    #[test]
    fn an_unknown_file_outlines_to_nothing() {
        let g = graph(
            vec![node("a", NodeKind::Function, "run", "src/a.ts", 1)],
            Vec::new(),
        );

        assert!(outline(&g, "src/missing.ts").is_empty());
    }

    #[test]
    fn rendering_prefers_the_signature() {
        let mut n = node("a", NodeKind::Function, "run", "src/a.ts", 7);
        n.signature = Some("export function run(x: number): void".to_string());
        let g = graph(vec![n], Vec::new());

        let text = render(&outline(&g, "src/a.ts"));

        assert!(text.contains("export function run(x: number): void"));
        assert!(text.contains("(7)"));
    }

    #[test]
    fn rendering_falls_back_to_kind_and_name() {
        let g = graph(
            vec![node("a", NodeKind::Function, "run", "src/a.ts", 1)],
            Vec::new(),
        );

        assert!(render(&outline(&g, "src/a.ts")).contains("function run"));
    }

    #[test]
    fn nesting_is_indented_in_the_rendered_output() {
        let g = graph(
            vec![
                node("c", NodeKind::Class, "Cache", "src/a.ts", 1),
                node("m", NodeKind::Method, "get", "src/a.ts", 5),
            ],
            vec![EdgeV1::new("c", "m", EdgeKind::Contains)],
        );

        let text = render(&outline(&g, "src/a.ts"));

        assert!(text.contains("\n  "));
    }

    #[test]
    fn outlining_is_deterministic() {
        let g = graph(
            vec![
                node("a", NodeKind::Function, "one", "src/a.ts", 1),
                node("b", NodeKind::Function, "two", "src/a.ts", 1),
            ],
            Vec::new(),
        );

        let first = render(&outline(&g, "src/a.ts"));
        for _ in 0..5 {
            assert_eq!(render(&outline(&g, "src/a.ts")), first);
        }
    }
}
