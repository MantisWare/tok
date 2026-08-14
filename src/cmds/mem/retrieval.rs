//! Handlers for the graph-backed retrieval commands.
//!
//! These are additive: `ask`, `skeleton`, `grep`, and `map` are new surfaces
//! that read `.tok/graph/`. The pre-existing `tok mem` subcommands keep their
//! SQLite queries and their output formats untouched, so upgrading cannot
//! change what an existing script or agent prompt sees.
//!
//! Output is written for an agent first and a human second: dense, one line per
//! result, location always in `file:line` form so it can be opened without a
//! follow-up query.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use colored::Colorize;

use super::savings;
use crate::graph::session::{self, Freshness};
use crate::query::{ask, federate, grep, index_file, map, skeleton};

pub fn run_ask(
    query: &str,
    path: &str,
    limit: usize,
    lexical: bool,
    in_path: Option<&str>,
    no_tests: bool,
) -> Result<i32> {
    let root = canonical(path)?;

    if federate::applies(&root) {
        return ask_workspace(&root, query, limit, lexical, in_path, no_tests);
    }

    let session = session::open(&root)?;
    let index =
        index_file::load_or_build(&crate::graph::store::GraphPaths::new(&root), &session.graph);

    let (scope_prefix, path_filter) = narrowing(in_path, &session.graph);

    let options = ask::AskOptions {
        mode: mode_for(lexical),
        limit,
        path_filter,
        exclude_tests: no_tests,
        scope_prefix,
    };

    let timer = savings::Savings::start(&root);
    let results = crate::query::scoped::ask(&session.graph, &index, query, &options);

    let rendered = render_ask(query, &results, staleness_note(session.freshness));
    print!("{rendered}");

    let report = timer.record(
        "tok mem ask",
        results.hits.iter().map(|s| s.hit.node.file.as_str()),
        &rendered,
    );
    print_footer(&report);

    Ok(0)
}

/// Print the savings line, when there is an honest one to print.
fn print_footer(report: &savings::Report) {
    if let Some(footer) = report.footer() {
        println!();
        println!("{}", footer.dimmed());
    }
}

/// Answer a query at a workspace parent by federating across its children.
fn ask_workspace(
    root: &Path,
    query: &str,
    limit: usize,
    lexical: bool,
    in_path: Option<&str>,
    no_tests: bool,
) -> Result<i32> {
    let members = crate::graph::workspace::members(root);
    let (child, inner) = match in_path {
        Some(value) => crate::graph::workspace::split_in(value, &members),
        None => (None, None),
    };

    let loaded = federate::load(root, child);

    if loaded.is_empty() {
        println!(
            "No indexed repositories under {}",
            root.display().to_string().cyan()
        );
        println!(
            "{}",
            "Run `tok mem index` inside each repository first.".dimmed()
        );
        return Ok(0);
    }

    let options = ask::AskOptions {
        mode: mode_for(lexical),
        limit,
        path_filter: None,
        exclude_tests: no_tests,
        scope_prefix: inner
            .map(crate::graph::scopes::normalize_prefix)
            .filter(|prefix| !prefix.is_empty()),
    };

    let timer = savings::Savings::start(root);
    let results = federate::ask(&loaded, query, &options);

    let rendered = render_ask(query, &results, String::new());
    print!("{rendered}");

    if !loaded.unindexed.is_empty() {
        println!();
        println!(
            "{} {}",
            "not indexed:".dimmed(),
            loaded.unindexed.join(", ").dimmed()
        );
    }

    // Paths are prefixed with the child repository, so they resolve from the
    // parent the same way the printed pointers do.
    let touched: Vec<String> = results.hits.iter().map(|scoped| scoped.path()).collect();
    let report = timer.record("tok mem ask", touched.iter().map(String::as_str), &rendered);
    print_footer(&report);

    Ok(0)
}

fn mode_for(lexical: bool) -> ask::Mode {
    if lexical {
        ask::Mode::Lexical
    } else {
        ask::Mode::Structural
    }
}

/// Render an answer to a string rather than printing it directly, so the
/// savings measurement can count exactly what the caller sees.
fn render_ask(
    query: &str,
    results: &crate::query::scoped::ScopedResults<'_>,
    note: String,
) -> String {
    use std::fmt::Write;

    let mut out = String::new();

    if results.is_empty() {
        let _ = writeln!(out, "No symbols matched {}", query.cyan());
        out.push_str(&also_matched(results));
        return out;
    }

    let _ = writeln!(
        out,
        "{} symbols for {}{}:\n",
        results.hits.len().to_string().bold(),
        query.cyan(),
        note
    );

    // Only worth the column when there is more than one place a hit could have
    // come from. In a single-project repo it would be the same label on every
    // line, which is pure cost.
    let show_scopes = results.per_scope.len() > 1;

    for scoped in &results.hits {
        let hit = &scoped.hit;
        // The marker earns its column: a rescued symbol shares no word with the
        // query, and without the cue its presence looks like a ranking bug.
        let marker = if hit.rescued { "~" } else { " " };
        let label = match scoped.label() {
            Some(label) if show_scopes => format!("[{label}] "),
            _ => String::new(),
        };

        let _ = writeln!(
            out,
            "{marker} {}{:<28} {} {}",
            label.dimmed(),
            hit.node.name.cyan(),
            format!("[{}]", hit.node.kind.as_str()).dimmed(),
            scoped.location().dimmed()
        );

        if let Some(signature) = &hit.node.signature {
            let _ = writeln!(out, "    {}", truncate(signature, 100).dimmed());
        }
    }

    if results.hits.iter().any(|s| s.hit.rescued) {
        let _ = writeln!(
            out,
            "\n{}",
            "~ reached through the graph, not by name".dimmed()
        );
    }

    out.push_str(&also_matched(results));
    out
}

/// Name the scopes that matched too weakly to include.
///
/// Without this the participation gate is invisible, and a user whose answer
/// lives in a demoted scope has no way to know it exists, let alone to ask for
/// it.
fn also_matched(results: &crate::query::scoped::ScopedResults<'_>) -> String {
    let Some(first) = results.also_matched.first() else {
        return String::new();
    };

    let labels: Vec<String> = results
        .also_matched
        .iter()
        .map(|prefix| crate::graph::scopes::scope_label(prefix))
        .collect();

    format!(
        "\n{} {} {}\n",
        "also matched:".dimmed(),
        labels.join(" "),
        format!("— narrow with --in {first}").dimmed()
    )
}

/// Interpret `--in` as either a scope prefix or a loose substring.
///
/// A value naming a real directory is treated as a prefix, which is what makes
/// `--in packages/api` confine the structural walk rather than merely filter
/// its output. Anything else stays a substring match, so `--in server` still
/// works the way someone typing it would expect.
fn narrowing(
    in_path: Option<&str>,
    graph: &crate::graph::GraphV1,
) -> (Option<String>, Option<String>) {
    let Some(raw) = in_path else {
        return (None, None);
    };

    let prefix = crate::graph::scopes::normalize_prefix(raw);
    if prefix.is_empty() {
        return (None, None);
    }

    let is_directory = graph
        .files
        .iter()
        .any(|file| crate::graph::scopes::path_under_prefix(&file.path, &prefix));

    if is_directory {
        (Some(prefix), None)
    } else {
        (None, Some(raw.to_string()))
    }
}

pub fn run_skeleton(file: Option<&str>, path: &str) -> Result<i32> {
    let root = canonical(path)?;
    let session = session::open(&root)?;

    let Some(file) = file else {
        let files = skeleton::files(&session.graph);
        println!("{} indexed files:", files.len().to_string().bold());
        for path in files {
            println!("  {path}");
        }
        return Ok(0);
    };

    let entries = skeleton::outline(&session.graph, file);

    if entries.is_empty() {
        println!("No symbols in {}", file.cyan());
        println!(
            "{}",
            "Run `tok mem skeleton` with no argument to list indexed files.".dimmed()
        );
        return Ok(0);
    }

    let timer = savings::Savings::start(&root);
    let mut out = format!("{} ({} symbols):\n", file.cyan(), entries.len());

    // Only present after `tok mem index --deep`. A one-line summary is worth
    // far more than its tokens when it saves opening the file at all.
    let enrichment =
        crate::graph::llm::Enrichment::load(&crate::graph::store::GraphPaths::new(&root));
    if let Some(summary) = enrichment.files.get(file) {
        out.push_str(&format!("{}\n", summary.dimmed()));
    }

    out.push('\n');
    out.push_str(&skeleton::render(&entries));
    print!("{out}");

    let report = timer.record("tok mem skeleton", [file], &out);
    print_footer(&report);

    Ok(0)
}

pub fn run_grep(
    pattern: &str,
    path: &str,
    regex: bool,
    case_sensitive: bool,
    in_path: Option<&str>,
    limit: usize,
) -> Result<i32> {
    let root = canonical(path)?;
    let session = session::open(&root)?;

    let options = grep::GrepOptions {
        regex,
        case_sensitive,
        path_filter: in_path.map(str::to_string),
        limit,
        ..grep::GrepOptions::default()
    };

    let results = grep::grep(&session.graph, &root, pattern, &options)
        .with_context(|| format!("Failed to search for {pattern}"))?;

    let total = grep::total(&results);
    if total == 0 {
        println!("No matches for {}", pattern.cyan());
        return Ok(1);
    }

    use std::fmt::Write;

    let timer = savings::Savings::start(&root);
    let mut out = format!(
        "{} matches in {} locations:\n",
        total.to_string().bold(),
        results.len()
    );

    for group in &results {
        out.push('\n');
        match group.node {
            Some(node) => {
                let _ = writeln!(
                    out,
                    "{} {} {}",
                    node.name.cyan(),
                    format!("[{}]", node.kind.as_str()).dimmed(),
                    node.location().dimmed()
                );
            }
            None => {
                let _ = writeln!(out, "{} {}", group.file.cyan(), "[file scope]".dimmed());
            }
        }

        for hit in &group.matches {
            let _ = writeln!(out, "  {:>5}  {}", hit.line.to_string().dimmed(), hit.text);
        }
    }

    print!("{out}");

    let report = timer.record(
        "tok mem grep",
        results.iter().map(|group| group.file.as_str()),
        &out,
    );
    print_footer(&report);

    Ok(0)
}

pub fn run_map(path: &str, max_dirs: usize, max_hubs: usize) -> Result<i32> {
    let root = canonical(path)?;
    let session = session::open(&root)?;

    let options = map::MapOptions {
        max_directories: max_dirs,
        max_hubs,
        ..map::MapOptions::default()
    };
    use std::fmt::Write;

    let timer = savings::Savings::start(&root);
    let overview = map::build(&session.graph, &options);

    let mut out = format!(
        "{} files, {} symbols, {} relationships{}\n",
        overview.file_count.to_string().bold(),
        overview.symbol_count.to_string().bold(),
        overview.edge_count.to_string().bold(),
        staleness_note(session.freshness)
    );

    if !overview.languages.is_empty() {
        let summary: Vec<String> = overview
            .languages
            .iter()
            .map(|(name, count)| format!("{name} {count}"))
            .collect();
        let _ = writeln!(out, "{}", summary.join("  ").dimmed());
    }

    if !overview.directories.is_empty() {
        let _ = writeln!(out, "\n{}", "Layout".bold());
        for dir in &overview.directories {
            let _ = writeln!(
                out,
                "  {:<34} {:>4} files {:>5} symbols  {}",
                dir.path,
                dir.files,
                dir.symbols,
                dir.language.dimmed()
            );
        }
    }

    if !overview.hubs.is_empty() {
        let _ = writeln!(out, "\n{}", "Most depended upon".bold());
        for hub in &overview.hubs {
            let _ = writeln!(
                out,
                "  {:>4} {:<28} {}",
                hub.dependents,
                hub.node.name.cyan(),
                hub.node.location().dimmed()
            );
        }
    }

    if !overview.entry_points.is_empty() {
        let _ = writeln!(out, "\n{}", "Entry points".bold());
        for node in &overview.entry_points {
            let _ = writeln!(
                out,
                "  {:<28} {}",
                node.name.cyan(),
                node.location().dimmed()
            );
        }
    }

    print!("{out}");

    // Only the files the map actually names. Counting the whole repository
    // would be the more flattering number and the less honest one.
    let named: Vec<&str> = overview
        .hubs
        .iter()
        .map(|hub| hub.node.file.as_str())
        .chain(overview.entry_points.iter().map(|node| node.file.as_str()))
        .collect();
    let report = timer.record("tok mem map", named, &out);
    print_footer(&report);

    Ok(0)
}

pub fn run_cards(path: &str) -> Result<i32> {
    let root = canonical(path)?;
    let session = session::open(&root)?;

    let stats = crate::markdown::write::write_all(&root, &session.graph)?;
    let dir = crate::markdown::write::markdown_dir(&root);

    println!(
        "{} written, {} unchanged, {} removed in {}",
        stats.written.to_string().bold(),
        stats.unchanged,
        stats.removed,
        dir.display()
    );

    for orphan in &stats.orphaned {
        println!(
            "  {} {} kept — its source is gone but it carries notes",
            "!".yellow(),
            orphan
        );
    }

    // A damaged file is the one case where the command did not do what was
    // asked, so it exits nonzero rather than reporting success with a caveat.
    for skipped in &stats.skipped {
        println!("  {} {}", "!".red(), skipped);
    }

    Ok(if stats.skipped.is_empty() { 0 } else { 1 })
}

pub fn run_check(path: &str, strict: bool) -> Result<i32> {
    let root = canonical(path)?;
    let session = session::open(&root)?;

    let report = crate::markdown::check::check(&root, &session.graph);

    if report.missing {
        println!("No generated markdown found. Run `tok mem cards` to create it.");
        return Ok(if strict { 1 } else { 0 });
    }

    if report.is_clean() {
        println!("{} markdown is up to date", "✓".green());
        return Ok(0);
    }

    if let Some(drift) = &report.index_drift {
        println!("{} {}", "index drift:".yellow().bold(), drift);
        println!("  Run `tok mem index`, then `tok mem cards`.");
        println!();
    }

    print_group("changed since generation", &report.content_drift);
    print_group("source file removed", &report.removed);
    print_group("not yet documented", &report.coverage);

    println!();
    println!(
        "{} stale entries. Run `tok mem cards` to regenerate.",
        report.total().to_string().bold()
    );

    Ok(if strict { 1 } else { 0 })
}

/// Run the optional LLM enrichment pass behind `tok mem index --deep`.
///
/// This is the one path in the subsystem that sends source code off the
/// machine and costs money per run, so it never happens without the flag.
pub fn run_enrich(path: &str) -> Result<i32> {
    let root = canonical(path)?;
    let session = session::open(&root)?;
    let config = crate::core::config::Config::load().unwrap_or_default();
    let settings = config.graph.llm.with_env_overrides();

    if !settings.enabled {
        println!(
            "{} --deep needs `[graph.llm] enabled = true` in config.toml. Nothing was sent.",
            "!".yellow()
        );
        return Ok(0);
    }

    let (enrichment, stats) = crate::graph::llm::enrich(&root, &session.graph, &settings, 1)?;

    println!(
        "{} {} files summarised, {} symbols explained, {} from cache",
        "✓".green(),
        stats.files_summarised,
        stats.symbols_explained,
        stats.cached
    );

    if stats.failed > 0 {
        println!(
            "{} {} calls failed and were skipped",
            "!".yellow(),
            stats.failed
        );
    }

    println!(
        "  {} entries stored in .tok/graph/",
        enrichment.files.len() + enrichment.symbols.len()
    );

    Ok(0)
}

fn print_group(label: &str, paths: &[String]) {
    if paths.is_empty() {
        return;
    }

    println!("{} ({})", label.yellow().bold(), paths.len());
    for path in paths {
        println!("  {path}");
    }
}

/// Resolve the repo root, failing with the path the user typed rather than a
/// bare OS error.
fn canonical(path: &str) -> Result<PathBuf> {
    let root = Path::new(path);
    root.canonicalize()
        .with_context(|| format!("Cannot read {path}"))
}

/// Tell the user when results may lag the working tree, and stay silent
/// otherwise. A freshness banner on every successful query is noise.
fn staleness_note(freshness: Freshness) -> String {
    match freshness {
        Freshness::Refreshed => String::new(),
        Freshness::Cached => format!(" {}", "(cached graph)".dimmed()),
        Freshness::Skipped(reason) => match reason {
            crate::graph::refresh::SkipReason::Disabled => {
                format!(" {}", "(refresh disabled)".dimmed())
            }
            crate::graph::refresh::SkipReason::Busy => {
                format!(" {}", "(index in progress)".dimmed())
            }
        },
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let head: String = text.chars().take(max).collect();
    format!("{head}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_path_names_itself_in_the_error() {
        let error = canonical("/definitely/not/here").expect_err("should fail");
        assert!(error.to_string().contains("/definitely/not/here"));
    }

    #[test]
    fn a_fresh_graph_gets_no_staleness_banner() {
        assert!(staleness_note(Freshness::Refreshed).is_empty());
    }

    #[test]
    fn a_skipped_refresh_says_why() {
        let busy = staleness_note(Freshness::Skipped(crate::graph::refresh::SkipReason::Busy));
        assert!(busy.contains("in progress"));

        let disabled = staleness_note(Freshness::Skipped(
            crate::graph::refresh::SkipReason::Disabled,
        ));
        assert!(disabled.contains("disabled"));
    }

    #[test]
    fn truncation_respects_character_boundaries() {
        let text = "é".repeat(300);
        assert_eq!(truncate(&text, 10).chars().count(), 11);
    }

    #[test]
    fn short_text_is_left_alone() {
        assert_eq!(truncate("short", 100), "short");
    }
}
