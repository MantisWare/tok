//! Symbol-aware search: matches attributed to the symbol that contains them.
//!
//! Plain grep answers "which lines contain this string". An agent almost always
//! wants "which *functions* contain this string", and then has to spend a
//! second read on each file to work that out. Using the graph's spans, the
//! owning symbol is known at match time, so results group by symbol and the
//! agent can act on the first response.
//!
//! Matching is literal and case-insensitive by default. A regex mode exists,
//! but the default is literal because an agent searching for `foo(` should not
//! have to know that `(` opens a capture group.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use regex::RegexBuilder;

use crate::graph::types::{GraphV1, NodeKind, NodeV1};

/// One matching line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// 1-based, matching how every editor and the graph's spans count.
    pub line: u32,
    pub text: String,
}

/// Matches grouped under the symbol that contains them.
#[derive(Debug, Clone)]
pub struct SymbolMatches<'a> {
    /// `None` when a match falls outside every symbol span — imports, module
    /// level constants, comments at the top of a file.
    pub node: Option<&'a NodeV1>,
    pub file: String,
    pub matches: Vec<Match>,
}

#[derive(Debug, Clone)]
pub struct GrepOptions {
    pub regex: bool,
    pub case_sensitive: bool,
    /// Restrict to files whose path contains this substring.
    pub path_filter: Option<String>,
    /// Cap on returned matches, so a common term cannot flood the context
    /// window it was supposed to conserve.
    pub limit: usize,
    /// Longer lines are truncated: a matched minified bundle line can be
    /// hundreds of thousands of characters.
    pub max_line_len: usize,
}

impl Default for GrepOptions {
    fn default() -> Self {
        Self {
            regex: false,
            case_sensitive: false,
            path_filter: None,
            limit: 100,
            max_line_len: 200,
        }
    }
}

/// Search the indexed files of a repo.
pub fn grep<'a>(
    graph: &'a GraphV1,
    repo_root: &Path,
    pattern: &str,
    options: &GrepOptions,
) -> Result<Vec<SymbolMatches<'a>>> {
    let escaped;
    let source = if options.regex {
        pattern
    } else {
        escaped = regex::escape(pattern);
        &escaped
    };

    let matcher = RegexBuilder::new(source)
        .case_insensitive(!options.case_sensitive)
        .size_limit(1 << 20)
        .build()?;

    // Keyed by (file, owning symbol id) so each group is a contiguous, sensible
    // unit for the reader. BTreeMap gives stable output ordering for free.
    let mut grouped: BTreeMap<(String, Option<String>), Vec<Match>> = BTreeMap::new();
    let mut found = 0usize;

    for entry in &graph.files {
        if let Some(filter) = &options.path_filter {
            if !entry.path.contains(filter.as_str()) {
                continue;
            }
        }

        let Ok(contents) = std::fs::read_to_string(repo_root.join(&entry.path)) else {
            // A file indexed earlier and deleted since is expected, not an
            // error: the next index run will drop it.
            continue;
        };

        let spans = symbols_in(graph, &entry.path);

        for (offset, line) in contents.lines().enumerate() {
            if found >= options.limit {
                break;
            }
            if !matcher.is_match(line) {
                continue;
            }

            let number = (offset + 1) as u32;
            let owner = owning_symbol(&spans, number).map(|n| n.id.clone());

            grouped
                .entry((entry.path.clone(), owner))
                .or_default()
                .push(Match {
                    line: number,
                    text: truncate(line.trim_end(), options.max_line_len),
                });
            found += 1;
        }

        if found >= options.limit {
            break;
        }
    }

    Ok(grouped
        .into_iter()
        .map(|((file, owner), matches)| SymbolMatches {
            node: owner.and_then(|id| graph.nodes.iter().find(|n| n.id == id)),
            file,
            matches,
        })
        .collect())
}

/// Symbols declared in a file, narrowest span last so the innermost owner wins.
fn symbols_in<'a>(graph: &'a GraphV1, file: &str) -> Vec<&'a NodeV1> {
    let mut nodes: Vec<&NodeV1> = graph
        .nodes
        .iter()
        .filter(|n| n.file == file)
        .filter(|n| n.kind != NodeKind::File && n.kind != NodeKind::Import)
        .collect();

    // Widest first, so a later scan picks the tightest enclosing span: a method
    // inside a class should be credited to the method.
    nodes.sort_by_key(|n| std::cmp::Reverse(n.span.end.saturating_sub(n.span.start)));
    nodes
}

fn owning_symbol<'a>(candidates: &[&'a NodeV1], line: u32) -> Option<&'a NodeV1> {
    candidates
        .iter()
        .rev()
        .find(|node| line >= node.span.start && line <= node.span.end)
        .copied()
}

fn truncate(line: &str, max: usize) -> String {
    if line.chars().count() <= max {
        return line.to_string();
    }

    // Truncating by chars rather than bytes; slicing a multi-byte codepoint
    // would panic.
    let head: String = line.chars().take(max).collect();
    format!("{head}…")
}

/// Total match count across groups, for the summary line.
pub fn total(results: &[SymbolMatches]) -> usize {
    results.iter().map(|r| r.matches.len()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::{FileEntryV1, Span};

    struct Fixture {
        dir: tempfile::TempDir,
        graph: GraphV1,
    }

    impl Fixture {
        fn new(files: &[(&str, &str)], nodes: Vec<NodeV1>) -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            let mut graph = GraphV1::new("repo", "test");

            for (path, contents) in files {
                let full = dir.path().join(path);
                if let Some(parent) = full.parent() {
                    std::fs::create_dir_all(parent).expect("mkdir");
                }
                std::fs::write(&full, contents).expect("write");

                graph.files.push(FileEntryV1 {
                    path: (*path).to_string(),
                    hash: "h".to_string(),
                    size: contents.len() as u64,
                    language: "typescript".to_string(),
                    node_count: 0,
                });
            }

            graph.nodes = nodes;
            graph.normalize();
            Self { dir, graph }
        }

        fn grep(&self, pattern: &str, options: &GrepOptions) -> Vec<SymbolMatches<'_>> {
            grep(&self.graph, self.dir.path(), pattern, options).expect("grep")
        }
    }

    fn node(id: &str, name: &str, file: &str, start: u32, end: u32) -> NodeV1 {
        NodeV1::new(
            id.to_string(),
            NodeKind::Function,
            name.to_string(),
            file.to_string(),
            Span::new(start, end),
        )
    }

    #[test]
    fn a_match_is_attributed_to_its_enclosing_symbol() {
        let fx = Fixture::new(
            &[("src/a.ts", "function run() {\n  const cache = 1;\n}\n")],
            vec![node("a", "run", "src/a.ts", 1, 3)],
        );

        let results = fx.grep("cache", &GrepOptions::default());

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].node.expect("owner").name, "run");
        assert_eq!(results[0].matches[0].line, 2);
    }

    #[test]
    fn a_match_outside_any_symbol_has_no_owner() {
        let fx = Fixture::new(
            &[(
                "src/a.ts",
                "import cache from './cache';\nfunction run() {}\n",
            )],
            vec![node("a", "run", "src/a.ts", 2, 2)],
        );

        let results = fx.grep("import", &GrepOptions::default());

        assert!(results[0].node.is_none());
    }

    /// The innermost span is the useful attribution: "it's in `Cache`" is far
    /// less actionable than "it's in `Cache.get`".
    #[test]
    fn the_narrowest_enclosing_symbol_wins() {
        let fx = Fixture::new(
            &[(
                "src/a.ts",
                "class Cache {\n  get() {\n    return 1;\n  }\n}\n",
            )],
            vec![
                node("outer", "Cache", "src/a.ts", 1, 5),
                node("inner", "get", "src/a.ts", 2, 4),
            ],
        );

        let results = fx.grep("return", &GrepOptions::default());

        assert_eq!(results[0].node.expect("owner").name, "get");
    }

    #[test]
    fn search_is_case_insensitive_by_default() {
        let fx = Fixture::new(
            &[("src/a.ts", "const Cache = 1;\n")],
            vec![node("a", "run", "src/a.ts", 1, 1)],
        );

        assert_eq!(total(&fx.grep("cache", &GrepOptions::default())), 1);
    }

    #[test]
    fn case_sensitivity_can_be_requested() {
        let fx = Fixture::new(
            &[("src/a.ts", "const Cache = 1;\n")],
            vec![node("a", "run", "src/a.ts", 1, 1)],
        );

        let options = GrepOptions {
            case_sensitive: true,
            ..GrepOptions::default()
        };

        assert_eq!(total(&fx.grep("cache", &options)), 0);
        assert_eq!(total(&fx.grep("Cache", &options)), 1);
    }

    /// The default has to be literal, or an agent searching for a call site
    /// gets a regex parse error instead of results.
    #[test]
    fn regex_metacharacters_are_literal_by_default() {
        let fx = Fixture::new(
            &[("src/a.ts", "run(1);\n")],
            vec![node("a", "run", "src/a.ts", 1, 1)],
        );

        assert_eq!(total(&fx.grep("run(", &GrepOptions::default())), 1);
    }

    #[test]
    fn regex_mode_interprets_the_pattern() {
        let fx = Fixture::new(
            &[("src/a.ts", "run1();\nrun2();\n")],
            vec![node("a", "run", "src/a.ts", 1, 2)],
        );

        let options = GrepOptions {
            regex: true,
            ..GrepOptions::default()
        };

        assert_eq!(total(&fx.grep(r"run\d", &options)), 2);
    }

    #[test]
    fn an_invalid_regex_is_reported_not_panicked() {
        let fx = Fixture::new(&[("src/a.ts", "x\n")], Vec::new());
        let options = GrepOptions {
            regex: true,
            ..GrepOptions::default()
        };

        assert!(grep(&fx.graph, fx.dir.path(), "(unclosed", &options).is_err());
    }

    #[test]
    fn the_limit_caps_returned_matches() {
        let body: String = (0..50).map(|_| "cache\n").collect();
        let fx = Fixture::new(&[("src/a.ts", &body)], Vec::new());

        let options = GrepOptions {
            limit: 5,
            ..GrepOptions::default()
        };

        assert_eq!(total(&fx.grep("cache", &options)), 5);
    }

    #[test]
    fn long_lines_are_truncated() {
        let long = format!("cache {}\n", "x".repeat(5_000));
        let fx = Fixture::new(&[("src/a.ts", &long)], Vec::new());

        let results = fx.grep("cache", &GrepOptions::default());

        assert!(results[0].matches[0].text.chars().count() <= 201);
    }

    /// A multi-byte character straddling the truncation point must not panic.
    #[test]
    fn truncation_respects_character_boundaries() {
        let long = format!("cache {}\n", "é".repeat(5_000));
        let fx = Fixture::new(&[("src/a.ts", &long)], Vec::new());

        assert_eq!(total(&fx.grep("cache", &GrepOptions::default())), 1);
    }

    #[test]
    fn a_path_filter_narrows_the_search() {
        let fx = Fixture::new(
            &[("src/a.ts", "cache\n"), ("test/a.ts", "cache\n")],
            Vec::new(),
        );

        let options = GrepOptions {
            path_filter: Some("test/".to_string()),
            ..GrepOptions::default()
        };
        let results = fx.grep("cache", &options);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file, "test/a.ts");
    }

    #[test]
    fn a_deleted_but_still_indexed_file_is_skipped() {
        let fx = Fixture::new(&[("src/a.ts", "cache\n")], Vec::new());
        std::fs::remove_file(fx.dir.path().join("src/a.ts")).expect("remove");

        assert!(fx.grep("cache", &GrepOptions::default()).is_empty());
    }

    #[test]
    fn results_are_ordered_deterministically() {
        let fx = Fixture::new(
            &[("src/b.ts", "cache\n"), ("src/a.ts", "cache\n")],
            Vec::new(),
        );

        let first: Vec<String> = fx
            .grep("cache", &GrepOptions::default())
            .iter()
            .map(|r| r.file.clone())
            .collect();

        assert_eq!(first, vec!["src/a.ts", "src/b.ts"]);
    }
}
