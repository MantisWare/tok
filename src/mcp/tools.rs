//! The six MCP tools, their schemas, and their handlers.
//!
//! Each is a thin wrapper over the same query layer the CLI uses, so there is
//! one implementation of ranking and one of rendering. The differences from the
//! CLI are deliberate and narrow:
//!
//! - **Output is plain text, never coloured.** ANSI escapes reach the model as
//!   literal noise that costs tokens and helps nobody.
//! - **Limits are lower.** A CLI user scrolls; a model pays for every token, so
//!   the defaults trade recall for economy and callers raise them explicitly.
//! - **Failure is a result, not an error.** "No matches" comes back as text so
//!   the model reads it and moves on, rather than as an RPC error that clients
//!   surface as a tool malfunction.
//!
//! Every tool is registered twice: under a `tok_*` name that says what it does,
//! and under the exact name graft published. The alias is spelled out per tool
//! rather than derived, because graft named its tools for the question they
//! answer (`graft_find_code`) and TOK names them for the operation (`tok_ask`) —
//! a prefix swap would produce `graft_ask`, which no existing config calls.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::graph::session;
use crate::graph::types::GraphV1;
use crate::markdown;
use crate::query::{ask, grep, index_file, map, skeleton, traverse};

/// Tool defaults, tuned for a model's context budget rather than a scrollback.
const DEFAULT_ASK_LIMIT: usize = 12;
const DEFAULT_GREP_LIMIT: usize = 40;
const DEFAULT_RELATION_DEPTH: usize = 2;
const DEFAULT_RELATION_LIMIT: usize = 40;

/// A tool as advertised by `tools/list`.
pub struct ToolSpec {
    /// Bare operation name; the advertised name is this behind `tok_`.
    pub name: &'static str,
    /// The name graft published for the same tool, advertised alongside.
    pub alias: &'static str,
    pub description: &'static str,
    pub schema: fn() -> Value,
}

impl ToolSpec {
    /// The `tok_`-prefixed name, allocated because the prefix is not in the
    /// literal.
    pub fn canonical(&self) -> String {
        format!("{CANONICAL_PREFIX}{}", self.name)
    }
}

pub const TOOLS: &[ToolSpec] = &[
    ToolSpec {
        name: "ask",
        alias: "graft_find_code",
        description: "Find the symbols worth reading to answer a question about the codebase. \
                      Combines text matching with the call graph, so it also returns callers and \
                      callees that share no words with the query. Prefer this over reading files.",
        schema: ask_schema,
    },
    ToolSpec {
        name: "skeleton",
        alias: "graft_file_api",
        description: "Outline a file: every declaration with its signature, and no bodies. \
                      Use this instead of reading a file when you need to know what it offers.",
        schema: skeleton_schema,
    },
    ToolSpec {
        name: "grep",
        alias: "graft_find_all",
        description: "Search source text, with each match attributed to the function or class \
                      that contains it. Literal by default; set regex to interpret the pattern.",
        schema: grep_schema,
    },
    ToolSpec {
        name: "map",
        alias: "graft_repo_map",
        description: "Repository overview: layout, languages, most depended-upon symbols, and \
                      entry points. Use this first in an unfamiliar codebase.",
        schema: map_schema,
    },
    ToolSpec {
        name: "relations",
        alias: "graft_trace_calls",
        description: "Walk the call graph from a symbol to find its callers, its callees, or \
                      both. Use this to work out the blast radius of a change.",
        schema: relations_schema,
    },
    ToolSpec {
        name: "check",
        alias: "graft_check_freshness",
        description: "Report whether the committed markdown under .tok/map/ still matches the \
                      code, and what to run to fix it.",
        schema: check_schema,
    },
];

/// Prefix used by the canonical tool names.
pub const CANONICAL_PREFIX: &str = "tok_";

/// Every advertised name, canonical first.
pub fn advertised() -> Vec<Value> {
    TOOLS
        .iter()
        .flat_map(|tool| {
            [tool.canonical(), tool.alias.to_string()]
                .into_iter()
                .map(move |name| {
                    json!({
                        "name": name,
                        "description": tool.description,
                        "inputSchema": (tool.schema)(),
                    })
                })
        })
        .collect()
}

/// Resolve either the canonical name or the graft alias.
pub fn resolve(name: &str) -> Option<&'static ToolSpec> {
    TOOLS
        .iter()
        .find(|tool| tool.alias == name || name.strip_prefix(CANONICAL_PREFIX) == Some(tool.name))
}

/// Run a tool, returning its text output and whether it represents a failure.
pub fn dispatch(name: &str, params: &Value, root: &Path) -> (String, bool) {
    let Some(tool) = resolve(name) else {
        return (format!("Unknown tool: {name}"), true);
    };

    match tool.name {
        "ask" => run_ask(params, root),
        "skeleton" => run_skeleton(params, root),
        "grep" => run_grep(params, root),
        "map" => run_map(root),
        "relations" => run_relations(params, root),
        "check" => run_check(root),
        _ => (format!("Unimplemented tool: {name}"), true),
    }
}

/// Open the graph, turning any failure into readable text.
///
/// A model cannot act on an `anyhow` chain, but it can act on "run tok mem
/// index", so errors are phrased as instructions.
fn open(root: &Path) -> Result<GraphV1, String> {
    session::open(root)
        .map(|s| s.graph)
        .map_err(|e| format!("{e}"))
}

fn run_ask(params: &Value, root: &Path) -> (String, bool) {
    let Some(query) = params.get("query").and_then(Value::as_str) else {
        return ("Missing required parameter: query".to_string(), true);
    };

    let narrow = params.get("in_path").and_then(Value::as_str);
    let limit = usize_param(params, "limit", DEFAULT_ASK_LIMIT);
    let mode = if params.get("lexical").and_then(Value::as_bool) == Some(true) {
        ask::Mode::Lexical
    } else {
        ask::Mode::Structural
    };
    let exclude_tests = params.get("no_tests").and_then(Value::as_bool) == Some(true);

    // A workspace parent holds no source of its own, so querying it directly
    // would answer nothing; federating is the only reading of the question that
    // makes sense there.
    if crate::query::federate::applies(root) {
        let members = crate::graph::workspace::members(root);
        let (child, inner) = match narrow {
            Some(value) => crate::graph::workspace::split_in(value, &members),
            None => (None, None),
        };

        let loaded = crate::query::federate::load(root, child);
        if loaded.is_empty() {
            return (
                "No indexed repositories in this workspace. Run `tok mem index` in each."
                    .to_string(),
                true,
            );
        }

        let options = ask::AskOptions {
            mode,
            limit,
            path_filter: None,
            exclude_tests,
            scope_prefix: inner
                .map(crate::graph::scopes::normalize_prefix)
                .filter(|prefix| !prefix.is_empty()),
        };

        let results = crate::query::federate::ask(&loaded, query, &options);
        return (render_ask(query, &results), false);
    }

    let graph = match open(root) {
        Ok(graph) => graph,
        Err(message) => return (message, true),
    };

    let index = index_file::load_or_build(&crate::graph::store::GraphPaths::new(root), &graph);

    let options = ask::AskOptions {
        mode,
        limit,
        path_filter: None,
        exclude_tests,
        scope_prefix: narrow
            .map(crate::graph::scopes::normalize_prefix)
            .filter(|prefix| !prefix.is_empty()),
    };

    let results = crate::query::scoped::ask(&graph, &index, query, &options);

    (render_ask(query, &results), false)
}

fn render_ask(query: &str, results: &crate::query::scoped::ScopedResults<'_>) -> String {
    if results.is_empty() {
        return format!("No symbols matched \"{query}\".");
    }

    let show_scopes = results.per_scope.len() > 1;

    let mut out = format!("{} symbols for \"{query}\":\n\n", results.hits.len());
    for scoped in &results.hits {
        let hit = &scoped.hit;
        let label = match scoped.label() {
            Some(label) if show_scopes => format!("[{label}] "),
            _ => String::new(),
        };

        out.push_str(&format!(
            "{label}{} [{}] {}\n",
            hit.node.name,
            hit.node.kind.as_str(),
            scoped.location()
        ));

        if let Some(signature) = &hit.node.signature {
            out.push_str(&format!("    {signature}\n"));
        }
        if hit.rescued {
            out.push_str("    (reached through the call graph, not by name)\n");
        }
    }

    if !results.also_matched.is_empty() {
        out.push_str(&format!(
            "\nAlso matched, more weakly: {}. Narrow with in_path.\n",
            results.also_matched.join(", ")
        ));
    }

    out
}

fn run_skeleton(params: &Value, root: &Path) -> (String, bool) {
    let graph = match open(root) {
        Ok(graph) => graph,
        Err(message) => return (message, true),
    };

    let Some(file) = params.get("file").and_then(Value::as_str) else {
        let files = skeleton::files(&graph);
        let mut out = format!("{} indexed files:\n", files.len());
        for path in files {
            out.push_str(&format!("{path}\n"));
        }
        return (out, false);
    };

    let entries = skeleton::outline(&graph, file);

    if entries.is_empty() {
        return (
            format!("No symbols in {file}. Call this tool with no file argument to list what is indexed."),
            false,
        );
    }

    (
        format!(
            "{file} ({} symbols):\n\n{}",
            entries.len(),
            skeleton::render(&entries)
        ),
        false,
    )
}

fn run_grep(params: &Value, root: &Path) -> (String, bool) {
    let Some(pattern) = params.get("pattern").and_then(Value::as_str) else {
        return ("Missing required parameter: pattern".to_string(), true);
    };

    let graph = match open(root) {
        Ok(graph) => graph,
        Err(message) => return (message, true),
    };

    let options = grep::GrepOptions {
        regex: params.get("regex").and_then(Value::as_bool) == Some(true),
        case_sensitive: params.get("case_sensitive").and_then(Value::as_bool) == Some(true),
        path_filter: params
            .get("in_path")
            .and_then(Value::as_str)
            .map(str::to_string),
        limit: usize_param(params, "limit", DEFAULT_GREP_LIMIT),
        ..grep::GrepOptions::default()
    };

    let results = match grep::grep(&graph, root, pattern, &options) {
        Ok(results) => results,
        // An invalid regex is the caller's mistake, and the message names it.
        Err(error) => return (format!("Invalid pattern: {error}"), true),
    };

    let total = grep::total(&results);
    if total == 0 {
        return (format!("No matches for \"{pattern}\"."), false);
    }

    let mut out = format!("{total} matches:\n");
    for group in &results {
        out.push('\n');
        match group.node {
            Some(node) => out.push_str(&format!(
                "{} [{}] {}\n",
                node.name,
                node.kind.as_str(),
                node.location()
            )),
            None => out.push_str(&format!("{} [file scope]\n", group.file)),
        }
        for hit in &group.matches {
            out.push_str(&format!("  {}: {}\n", hit.line, hit.text));
        }
    }

    (out, false)
}

fn run_map(root: &Path) -> (String, bool) {
    let graph = match open(root) {
        Ok(graph) => graph,
        Err(message) => return (message, true),
    };

    let overview = map::build(&graph, &map::MapOptions::default());

    let mut out = format!(
        "{} files, {} symbols, {} relationships\n",
        overview.file_count, overview.symbol_count, overview.edge_count
    );

    if !overview.languages.is_empty() {
        let summary: Vec<String> = overview
            .languages
            .iter()
            .map(|(name, count)| format!("{name} {count}"))
            .collect();
        out.push_str(&format!("Languages: {}\n", summary.join(", ")));
    }

    if !overview.directories.is_empty() {
        out.push_str("\nLayout:\n");
        for dir in &overview.directories {
            out.push_str(&format!(
                "  {} — {} files, {} symbols ({})\n",
                dir.path, dir.files, dir.symbols, dir.language
            ));
        }
    }

    if !overview.hubs.is_empty() {
        out.push_str("\nMost depended upon:\n");
        for hub in &overview.hubs {
            out.push_str(&format!(
                "  {} — {} dependents ({})\n",
                hub.node.name,
                hub.dependents,
                hub.node.location()
            ));
        }
    }

    if !overview.entry_points.is_empty() {
        out.push_str("\nEntry points:\n");
        for node in &overview.entry_points {
            out.push_str(&format!("  {} ({})\n", node.name, node.location()));
        }
    }

    (out, false)
}

fn run_relations(params: &Value, root: &Path) -> (String, bool) {
    let Some(symbol) = params.get("symbol").and_then(Value::as_str) else {
        return ("Missing required parameter: symbol".to_string(), true);
    };

    let graph = match open(root) {
        Ok(graph) => graph,
        Err(message) => return (message, true),
    };

    let direction = params
        .get("direction")
        .and_then(Value::as_str)
        .and_then(traverse::Direction::parse)
        .unwrap_or(traverse::Direction::Both);

    let depth = usize_param(params, "depth", DEFAULT_RELATION_DEPTH).clamp(1, 5);

    // A name is what a model has; an id is what the graph indexes by. Resolve
    // by name and report the ambiguity rather than silently picking one.
    let matches: Vec<&crate::graph::types::NodeV1> =
        graph.nodes.iter().filter(|n| n.name == symbol).collect();

    let Some(start) = matches.first() else {
        return (
            format!("No symbol named \"{symbol}\". Use the ask tool to find the right name."),
            false,
        );
    };

    let reached = traverse::walk(&graph, &start.id, direction, depth, &[]);

    let mut out = format!(
        "{} ({}) — {} related symbols within {depth} hops ({}):\n\n",
        start.name,
        start.location(),
        reached.len().min(DEFAULT_RELATION_LIMIT),
        direction.as_str()
    );

    if matches.len() > 1 {
        out.push_str(&format!(
            "Note: {} symbols share this name; showing the first.\n\n",
            matches.len()
        ));
    }

    for item in reached.iter().take(DEFAULT_RELATION_LIMIT) {
        let Some(node) = graph.node(&item.id) else {
            continue;
        };
        out.push_str(&format!(
            "  {} [{}] {} — {} hop(s), via {}\n",
            node.name,
            node.kind.as_str(),
            node.location(),
            item.depth,
            item.via.as_str()
        ));
    }

    if reached.is_empty() {
        out.push_str("  (nothing connects to this symbol)\n");
    }

    (out, false)
}

fn run_check(root: &Path) -> (String, bool) {
    let graph = match open(root) {
        Ok(graph) => graph,
        Err(message) => return (message, true),
    };

    let report = markdown::check::check(root, &graph);

    if report.missing {
        return (
            "No generated markdown found. Run `tok mem cards` to create it.".to_string(),
            false,
        );
    }
    if report.is_clean() {
        return ("Markdown is up to date.".to_string(), false);
    }

    let mut out = String::new();
    if let Some(drift) = &report.index_drift {
        out.push_str(&format!(
            "Index drift: {drift}. Run `tok mem index`, then `tok mem cards`.\n\n"
        ));
    }

    let mut group = |label: &str, paths: &[String]| {
        if paths.is_empty() {
            return;
        }
        out.push_str(&format!("{label} ({}):\n", paths.len()));
        for path in paths {
            out.push_str(&format!("  {path}\n"));
        }
    };

    group("Changed since generation", &report.content_drift);
    group("Source file removed", &report.removed);
    group("Not yet documented", &report.coverage);

    out.push_str("\nRun `tok mem cards` to regenerate.\n");
    (out, false)
}

fn usize_param(params: &Value, name: &str, default: usize) -> usize {
    params
        .get(name)
        .and_then(Value::as_u64)
        .map(|v| v as usize)
        .filter(|v| *v > 0)
        .unwrap_or(default)
}

/// The repository the server answers questions about.
///
/// An explicit directory wins over the working one, because clients that launch
/// the server from their own install path would otherwise index that instead of
/// the project.
pub fn repo_root(dir: Option<&str>) -> PathBuf {
    match dir {
        Some(dir) => PathBuf::from(dir),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    }
}

// ------------------------------------------------------------- schemas

fn ask_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": { "type": "string", "description": "What you want to understand" },
            "limit": { "type": "integer", "description": "Max results (default 12)" },
            "lexical": { "type": "boolean", "description": "Skip the graph walk and match text only" },
            "in_path": { "type": "string", "description": "Only files whose path contains this" },
            "no_tests": { "type": "boolean", "description": "Exclude test files entirely" }
        },
        "required": ["query"]
    })
}

fn skeleton_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "file": {
                "type": "string",
                "description": "Repo-relative file to outline; omit to list indexed files"
            }
        }
    })
}

fn grep_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "pattern": { "type": "string", "description": "Text to find" },
            "regex": { "type": "boolean", "description": "Interpret the pattern as a regex" },
            "case_sensitive": { "type": "boolean", "description": "Match case exactly" },
            "in_path": { "type": "string", "description": "Only files whose path contains this" },
            "limit": { "type": "integer", "description": "Max matches (default 40)" }
        },
        "required": ["pattern"]
    })
}

fn map_schema() -> Value {
    json!({ "type": "object", "properties": {} })
}

fn relations_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "symbol": { "type": "string", "description": "Symbol name to walk from" },
            "direction": {
                "type": "string",
                "enum": ["in", "out", "both"],
                "description": "in = callers, out = callees, both = blast radius"
            },
            "depth": { "type": "integer", "description": "Hops to follow, 1-5 (default 2)" }
        },
        "required": ["symbol"]
    })
}

fn check_schema() -> Value {
    json!({ "type": "object", "properties": {} })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        for (path, contents) in files {
            let full = dir.path().join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).expect("mkdir");
            }
            std::fs::write(full, contents).expect("write");
        }
        dir
    }

    fn fixture() -> tempfile::TempDir {
        repo(&[(
            "a.rs",
            "pub fn helper() {}\npub fn caller() { helper(); }\n",
        )])
    }

    #[test]
    fn every_tool_is_advertised_under_both_prefixes() {
        let names: Vec<String> = advertised()
            .iter()
            .map(|t| t["name"].as_str().expect("name").to_string())
            .collect();

        assert_eq!(names.len(), TOOLS.len() * 2);
        assert!(names.contains(&"tok_ask".to_string()));
        assert!(names.contains(&"graft_find_code".to_string()));
    }

    /// The alias exists so agents configured against graft keep working.
    #[test]
    fn both_prefixes_resolve_to_the_same_tool() {
        let canonical = resolve("tok_ask").expect("canonical");
        let alias = resolve("graft_find_code").expect("alias");

        assert_eq!(canonical.name, alias.name);
    }

    #[test]
    fn an_unprefixed_or_unknown_name_does_not_resolve() {
        assert!(resolve("ask").is_none());
        assert!(resolve("tok_nonexistent").is_none());
        assert!(resolve("other_ask").is_none());
        // The names a prefix swap would have produced. An agent config naming
        // these was never valid against graft either, so accepting them would
        // only hide a typo.
        assert!(resolve("graft_ask").is_none());
        assert!(resolve("graft_map").is_none());
    }

    /// Pinned against graft's published names. Changing one silently breaks
    /// every agent config that was written for graft, which is the single
    /// thing the alias exists to prevent.
    #[test]
    fn the_aliases_are_the_names_graft_published() {
        let aliases: Vec<&str> = TOOLS.iter().map(|t| t.alias).collect();

        assert_eq!(
            aliases,
            vec![
                "graft_find_code",
                "graft_file_api",
                "graft_find_all",
                "graft_repo_map",
                "graft_trace_calls",
                "graft_check_freshness",
            ]
        );
    }

    #[test]
    fn every_advertised_tool_has_a_schema_and_a_description() {
        for tool in advertised() {
            assert!(!tool["description"]
                .as_str()
                .expect("description")
                .is_empty());
            assert_eq!(tool["inputSchema"]["type"], "object");
        }
    }

    #[test]
    fn ask_finds_a_symbol() {
        let dir = fixture();

        let (text, is_error) = dispatch("tok_ask", &json!({"query": "helper"}), dir.path());

        assert!(!is_error);
        assert!(text.contains("helper"), "output:\n{text}");
    }

    #[test]
    fn ask_without_a_query_reports_the_missing_parameter() {
        let dir = fixture();

        let (text, is_error) = dispatch("tok_ask", &json!({}), dir.path());

        assert!(is_error);
        assert!(text.contains("query"));
    }

    /// Finding nothing is a valid answer, not a tool malfunction.
    #[test]
    fn no_results_is_not_an_error() {
        let dir = fixture();

        let (text, is_error) = dispatch("tok_ask", &json!({"query": "zzzznope"}), dir.path());

        assert!(!is_error);
        assert!(text.contains("No symbols matched"));
    }

    #[test]
    fn skeleton_lists_files_when_given_none() {
        let dir = fixture();

        let (text, is_error) = dispatch("tok_skeleton", &json!({}), dir.path());

        assert!(!is_error);
        assert!(text.contains("a.rs"));
    }

    #[test]
    fn skeleton_outlines_a_named_file() {
        let dir = fixture();

        let (text, _) = dispatch("tok_skeleton", &json!({"file": "a.rs"}), dir.path());

        assert!(text.contains("helper"), "output:\n{text}");
    }

    #[test]
    fn grep_attributes_matches_to_symbols() {
        let dir = fixture();

        let (text, is_error) = dispatch("tok_grep", &json!({"pattern": "helper"}), dir.path());

        assert!(!is_error);
        assert!(text.contains("matches"), "output:\n{text}");
    }

    #[test]
    fn an_invalid_regex_is_reported_as_a_tool_error() {
        let dir = fixture();

        let (text, is_error) = dispatch(
            "tok_grep",
            &json!({"pattern": "(unclosed", "regex": true}),
            dir.path(),
        );

        assert!(is_error);
        assert!(text.contains("Invalid pattern"));
    }

    #[test]
    fn map_summarizes_the_repository() {
        let dir = fixture();

        let (text, is_error) = dispatch("tok_map", &json!({}), dir.path());

        assert!(!is_error);
        assert!(text.contains("files"));
        assert!(text.contains("symbols"));
    }

    #[test]
    fn relations_walks_from_a_named_symbol() {
        let dir = fixture();

        let (text, is_error) = dispatch(
            "tok_relations",
            &json!({"symbol": "helper", "direction": "in"}),
            dir.path(),
        );

        assert!(!is_error);
        assert!(text.contains("caller"), "output:\n{text}");
    }

    #[test]
    fn relations_on_an_unknown_symbol_suggests_the_ask_tool() {
        let dir = fixture();

        let (text, is_error) = dispatch("tok_relations", &json!({"symbol": "nope"}), dir.path());

        assert!(!is_error);
        assert!(text.contains("ask tool"));
    }

    /// An unbounded walk would return most of a real repository.
    #[test]
    fn relation_depth_is_clamped() {
        let dir = fixture();

        let (text, _) = dispatch(
            "tok_relations",
            &json!({"symbol": "helper", "depth": 99}),
            dir.path(),
        );

        assert!(text.contains("within 5 hops"), "output:\n{text}");
    }

    #[test]
    fn check_reports_missing_markdown() {
        let dir = fixture();

        let (text, is_error) = dispatch("tok_check", &json!({}), dir.path());

        assert!(!is_error);
        assert!(text.contains("tok mem cards"));
    }

    #[test]
    fn an_unknown_tool_is_an_error() {
        let dir = fixture();

        let (text, is_error) = dispatch("tok_nope", &json!({}), dir.path());

        assert!(is_error);
        assert!(text.contains("Unknown tool"));
    }

    /// ANSI escapes reach the model as literal noise it pays tokens for.
    #[test]
    fn tool_output_carries_no_ansi_escapes() {
        let dir = fixture();

        for (name, params) in [
            ("tok_ask", json!({"query": "helper"})),
            ("tok_map", json!({})),
            ("tok_skeleton", json!({"file": "a.rs"})),
        ] {
            let (text, _) = dispatch(name, &params, dir.path());
            assert!(!text.contains('\u{1b}'), "{name} emitted an escape code");
        }
    }

    #[test]
    fn a_limit_of_zero_falls_back_to_the_default() {
        assert_eq!(usize_param(&json!({"limit": 0}), "limit", 12), 12);
        assert_eq!(usize_param(&json!({}), "limit", 12), 12);
        assert_eq!(usize_param(&json!({"limit": 3}), "limit", 12), 3);
    }
}
