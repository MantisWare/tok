//! `INDEX.md`: the entry point into the generated card set.
//!
//! Without it the cards directory is a flat pile of files whose names encode
//! paths, which is navigable by grep but not by reading. The index groups cards
//! by directory and leads with the repository's shape, so an agent pointed at
//! `INDEX.md` can find the right card in one hop instead of listing a
//! directory and guessing from filenames.

use std::collections::BTreeMap;

use crate::graph::types::GraphV1;
use crate::markdown::cards::Card;
use crate::markdown::frontmatter::Frontmatter;
use crate::query::map::{self, MapOptions};

/// How many hub symbols the index highlights before it stops being a summary.
const HUB_LIMIT: usize = 10;

/// The index, split the same way a card is: frontmatter for tools, body for the
/// generated block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Index {
    pub frontmatter: String,
    pub body: String,
}

/// Render the index for a set of cards.
pub fn build(graph: &GraphV1, cards: &[Card]) -> Index {
    let mut frontmatter = Frontmatter::new();
    frontmatter
        .set("tok_kind", "index")
        .set("files", cards.len().to_string());

    let mut out = String::from("# Code map\n\n");

    let overview = map::build(
        graph,
        &MapOptions {
            max_hubs: HUB_LIMIT,
            ..MapOptions::default()
        },
    );

    out.push_str(&format!(
        "{} files, {} symbols, {} relationships.\n",
        overview.file_count, overview.symbol_count, overview.edge_count
    ));

    if !overview.languages.is_empty() {
        let summary: Vec<String> = overview
            .languages
            .iter()
            .map(|(name, count)| format!("{name} ({count})"))
            .collect();
        out.push_str(&format!("\nLanguages: {}.\n", summary.join(", ")));
    }

    out.push_str(&render_hubs(&overview));
    out.push_str(&render_directories(cards));

    Index {
        frontmatter: frontmatter.render(),
        body: out.trim_end().to_string(),
    }
}

fn render_hubs(overview: &map::RepoMap) -> String {
    if overview.hubs.is_empty() {
        return String::new();
    }

    let mut out = String::from("\n## Most depended upon\n\n");
    for hub in &overview.hubs {
        out.push_str(&format!(
            "- **{}** — {} dependents ({})\n",
            hub.node.name,
            hub.dependents,
            hub.node.location()
        ));
    }
    out
}

fn render_directories(cards: &[Card]) -> String {
    if cards.is_empty() {
        return "\nNothing indexed yet. Run `tok mem index`.\n".to_string();
    }

    let mut grouped: BTreeMap<&str, Vec<&Card>> = BTreeMap::new();
    for card in cards {
        grouped
            .entry(parent_dir(&card.path))
            .or_default()
            .push(card);
    }

    let mut out = String::from("\n## Files\n");

    for (directory, cards) in grouped {
        out.push_str(&format!("\n### {directory}\n\n"));
        for card in cards {
            out.push_str(&format!(
                "- [{}]({}) \n",
                file_name(&card.path),
                card.filename
            ));
        }
    }

    out
}

fn parent_dir(path: &str) -> &str {
    match path.rfind('/') {
        Some(index) => &path[..index],
        None => ".",
    }
}

fn file_name(path: &str) -> &str {
    match path.rfind('/') {
        Some(index) => &path[index + 1..],
        None => path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::{EdgeKind, EdgeV1, FileEntryV1, NodeKind, NodeV1, Span};
    use crate::markdown::cards;

    fn file(path: &str) -> FileEntryV1 {
        FileEntryV1 {
            path: path.to_string(),
            hash: "h".to_string(),
            size: 1,
            language: "typescript".to_string(),
            node_count: 1,
        }
    }

    fn node(id: &str, name: &str, path: &str) -> NodeV1 {
        NodeV1::new(
            id.to_string(),
            NodeKind::Function,
            name.to_string(),
            path.to_string(),
            Span::new(1, 5),
        )
    }

    fn graph(nodes: Vec<NodeV1>, edges: Vec<EdgeV1>, files: Vec<FileEntryV1>) -> GraphV1 {
        let mut g = GraphV1::new("repo", "test");
        g.nodes = nodes;
        g.edges = edges;
        g.files = files;
        g.normalize();
        g
    }

    fn render(g: &GraphV1) -> String {
        build(g, &cards::build_all(g)).body
    }

    #[test]
    fn the_index_summarizes_the_repository() {
        let g = graph(
            vec![node("a", "one", "src/a.ts")],
            Vec::new(),
            vec![file("src/a.ts")],
        );

        let text = render(&g);

        assert!(text.contains("# Code map"));
        assert!(text.contains("1 files, 1 symbols"));
    }

    #[test]
    fn files_are_grouped_by_directory() {
        let g = graph(
            Vec::new(),
            Vec::new(),
            vec![file("src/core/a.ts"), file("docs/b.ts")],
        );

        let text = render(&g);

        assert!(text.contains("### src/core"));
        assert!(text.contains("### docs"));
    }

    #[test]
    fn each_file_links_to_its_card() {
        let g = graph(Vec::new(), Vec::new(), vec![file("src/cache.ts")]);

        let text = render(&g);

        assert!(text.contains("[cache.ts](src-cache-ts.md)"));
    }

    #[test]
    fn hubs_are_highlighted() {
        let g = graph(
            vec![
                node("hub", "shared", "src/a.ts"),
                node("x", "x", "src/b.ts"),
            ],
            vec![EdgeV1::new("x", "hub", EdgeKind::Calls)],
            vec![file("src/a.ts"), file("src/b.ts")],
        );

        let text = render(&g);

        assert!(text.contains("## Most depended upon"));
        assert!(text.contains("**shared**"));
    }

    #[test]
    fn an_empty_repository_says_what_to_do_next() {
        let g = graph(Vec::new(), Vec::new(), Vec::new());

        assert!(render(&g).contains("tok mem index"));
    }

    #[test]
    fn a_root_level_file_is_grouped_under_dot() {
        let g = graph(Vec::new(), Vec::new(), vec![file("index.ts")]);

        assert!(render(&g).contains("### ."));
    }

    #[test]
    fn the_index_carries_frontmatter_outside_its_body() {
        let g = graph(Vec::new(), Vec::new(), vec![file("src/a.ts")]);

        let index = build(&g, &cards::build_all(&g));

        assert!(index.frontmatter.starts_with("---\n"));
        assert!(index.frontmatter.contains("tok_kind: index"));
        assert!(index.body.starts_with("# Code map"));
    }

    #[test]
    fn index_generation_is_deterministic() {
        let g = graph(
            Vec::new(),
            Vec::new(),
            vec![file("src/b.ts"), file("src/a.ts")],
        );

        let first = render(&g);
        for _ in 0..5 {
            assert_eq!(render(&g), first);
        }
    }
}
