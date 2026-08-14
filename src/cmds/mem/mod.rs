//! CLI command handlers for `tok mem` — structural code memory.

use anyhow::{Context, Result};
use colored::Colorize;

use crate::mem::{db, evolution, graph, indexer, quality, search, symbols};
use crate::MemCommands;

mod retrieval;
mod savings;

/// Dispatch a `tok mem <subcommand>` to the correct handler.
pub fn dispatch_mem(cmd: MemCommands) -> Result<i32> {
    match cmd {
        MemCommands::Index {
            path,
            repo_id,
            branch,
            incremental,
            clear,
            deep,
        } => run_index(&path, repo_id.as_deref(), &branch, incremental, clear, deep),

        MemCommands::Search {
            query,
            repo_id,
            kind,
            limit,
        } => run_search(&query, repo_id.as_deref(), kind.as_deref(), limit),

        MemCommands::Find {
            name,
            fuzzy,
            repo_id,
            kind,
            limit,
        } => run_find(&name, fuzzy, repo_id.as_deref(), kind.as_deref(), limit),

        MemCommands::Context { name, repo_id } => run_context(&name, repo_id.as_deref()),

        MemCommands::Relations {
            name,
            query_type,
            depth,
            limit,
            repo_id,
        } => run_relations(&name, &query_type, depth, limit, repo_id.as_deref()),

        MemCommands::Impact {
            name,
            direction,
            depth,
            limit,
            repo_id,
        } => run_impact(&name, &direction, depth, limit, repo_id.as_deref()),

        MemCommands::Repos => run_repos(),

        MemCommands::Status { repo_id } => run_status(repo_id.as_deref()),

        MemCommands::Forget { repo_id } => run_forget(&repo_id),

        MemCommands::Evolution {
            from,
            to,
            mode,
            repo_id,
            max_symbols,
        } => run_evolution(&from, &to, &mode, repo_id.as_deref(), max_symbols),

        MemCommands::Timeline { name, repo_id } => run_timeline(&name, repo_id.as_deref()),

        MemCommands::Changes {
            since_episode,
            since_time,
            repo_id,
        } => run_changes(
            since_episode.as_deref(),
            since_time.as_deref(),
            repo_id.as_deref(),
        ),

        MemCommands::Detect { files, repo_id } => run_detect(&files, repo_id.as_deref()),

        MemCommands::Central { repo_id, limit } => run_central(repo_id.as_deref(), limit),

        MemCommands::Bridges { repo_id, limit } => run_bridges(repo_id.as_deref(), limit),

        MemCommands::Communities {
            repo_id,
            min_size,
            limit,
        } => run_communities(repo_id.as_deref(), min_size, limit),

        MemCommands::DeadCode {
            repo_id,
            include_tests,
            limit,
        } => run_dead_code(repo_id.as_deref(), include_tests, limit),

        MemCommands::Complexity {
            repo_id,
            limit,
            min_complexity,
        } => run_complexity(repo_id.as_deref(), limit, min_complexity),

        MemCommands::Ask {
            query,
            path,
            limit,
            lexical,
            in_path,
            no_tests,
        } => retrieval::run_ask(&query, &path, limit, lexical, in_path.as_deref(), no_tests),

        MemCommands::Skeleton { file, path } => retrieval::run_skeleton(file.as_deref(), &path),

        MemCommands::Grep {
            pattern,
            path,
            regex,
            case_sensitive,
            in_path,
            limit,
        } => retrieval::run_grep(
            &pattern,
            &path,
            regex,
            case_sensitive,
            in_path.as_deref(),
            limit,
        ),

        MemCommands::Map {
            path,
            max_dirs,
            max_hubs,
        } => retrieval::run_map(&path, max_dirs, max_hubs),

        MemCommands::Cards { path } => retrieval::run_cards(&path),

        MemCommands::Check { path, strict } => retrieval::run_check(&path, strict),
    }
}

fn run_index(
    path: &str,
    repo_id: Option<&str>,
    branch: &str,
    incremental: bool,
    clear: bool,
    deep: bool,
) -> Result<i32> {
    let root =
        std::fs::canonicalize(path).with_context(|| format!("Cannot resolve path: {path}"))?;

    // A workspace parent holds no source of its own. Indexing it as one
    // repository would produce a graph whose paths belong to nobody, so each
    // child is indexed on its own terms and the parent stores only the list.
    if crate::graph::workspace::is_workspace_root(&root) {
        return run_index_workspace(&root, branch, incremental, clear, deep);
    }

    let conn = db::open()?;

    let resolved_repo_id = match repo_id {
        Some(id) => id.to_string(),
        None => {
            let p = std::fs::canonicalize(path)
                .with_context(|| format!("Cannot resolve path: {}", path))?;
            p.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string()
        }
    };

    if clear {
        db::clear_repo_symbols(&conn, &resolved_repo_id)?;
        eprintln!(
            "{} Cleared existing data for {}",
            "✓".green(),
            resolved_repo_id.bold()
        );
    }

    eprintln!(
        "{} Indexing {} as {}…",
        "→".cyan(),
        path.bold(),
        resolved_repo_id.bold()
    );

    let stats = indexer::index_directory(&conn, path, &resolved_repo_id, branch, incremental)?;

    println!(
        "{repo} {files} files scanned, {parsed} parsed, {syms} symbols, {edges} edges",
        repo = resolved_repo_id.bold(),
        files = stats.files_scanned,
        parsed = stats.files_parsed,
        syms = stats.symbols_inserted,
        edges = stats.edges_inserted,
    );

    if !stats.errors.is_empty() {
        eprintln!(
            "{} {} errors during indexing",
            "⚠".yellow(),
            stats.errors.len()
        );
    }

    if deep {
        retrieval::run_enrich(path)?;
    }

    Ok(0)
}

/// Index every repository under a workspace parent.
///
/// Each child is built exactly as it would be alone, so a repository's graph
/// never depends on whether it was indexed through its parent.
fn run_index_workspace(
    root: &std::path::Path,
    branch: &str,
    incremental: bool,
    clear: bool,
    deep: bool,
) -> Result<i32> {
    let workspace = crate::graph::workspace::refresh(root)?;

    if workspace.is_empty() {
        eprintln!("{} No repositories found to index", "⚠".yellow());
        return Ok(0);
    }

    eprintln!(
        "{} Workspace: {} repositories",
        "→".cyan(),
        workspace.children.len().to_string().bold()
    );

    let mut failed = Vec::new();

    for child in &workspace.children {
        let path = root.join(child);
        let Some(path) = path.to_str() else {
            failed.push(child.clone());
            continue;
        };

        // One child's failure must not stop the rest: a broken repository in
        // the workspace is a reason to report it, not to leave the other four
        // unsearchable.
        if let Err(e) = run_index(path, Some(child), branch, incremental, clear, deep) {
            eprintln!("{} {child}: {e}", "⚠".yellow());
            failed.push(child.clone());
        }
    }

    if !failed.is_empty() {
        eprintln!(
            "{} {} of {} repositories failed",
            "⚠".yellow(),
            failed.len(),
            workspace.children.len()
        );
    }

    Ok(0)
}

fn run_search(query: &str, repo_id: Option<&str>, kind: Option<&str>, limit: usize) -> Result<i32> {
    let conn = db::open()?;
    let kind_filter = kind.and_then(symbols::SymbolKind::from_str);

    let results = search::fts_search(&conn, query, repo_id, kind_filter, limit)?;

    if results.is_empty() {
        println!("No results for {:?}", query);
        return Ok(0);
    }

    println!(
        "{} results for {:?}:",
        results.len().to_string().bold(),
        query
    );
    println!();

    for r in &results {
        print_symbol_line(&r.symbol);
    }

    Ok(0)
}

fn run_find(
    name: &str,
    fuzzy: bool,
    repo_id: Option<&str>,
    kind: Option<&str>,
    limit: usize,
) -> Result<i32> {
    let conn = db::open()?;
    let kind_filter = kind.and_then(symbols::SymbolKind::from_str);

    let results = if fuzzy {
        db::find_symbol_fuzzy(&conn, name, repo_id, limit)?
    } else {
        db::find_symbol_by_name(&conn, name, repo_id, kind_filter, limit)?
    };

    if results.is_empty() {
        println!("No symbols found for {:?}", name);
        return Ok(0);
    }

    println!("{} symbols:", results.len().to_string().bold());
    println!();

    for sym in &results {
        print_symbol_line(sym);
    }

    Ok(0)
}

fn run_context(name: &str, repo_id: Option<&str>) -> Result<i32> {
    let conn = db::open()?;

    let sym = match resolve_symbol(&conn, name, repo_id)? {
        Some(s) => s,
        None => {
            println!("Symbol {:?} not found", name);
            return Ok(1);
        }
    };

    println!("{}", "Symbol".bold().underline());
    print_symbol_detail(&sym);
    println!();

    let callers = graph::find_callers(&conn, &sym.id, 20)?;
    if !callers.is_empty() {
        println!("{} ({}):", "Callers".bold(), callers.len());
        for c in &callers {
            println!("  ← {} ({}:{})", c.name.cyan(), c.file_path, c.line_start);
        }
        println!();
    }

    let callees = graph::find_callees(&conn, &sym.id, 20)?;
    if !callees.is_empty() {
        println!("{} ({}):", "Callees".bold(), callees.len());
        for c in &callees {
            println!("  → {} ({}:{})", c.name.cyan(), c.file_path, c.line_start);
        }
        println!();
    }

    let all_edges = db::get_edges_for_symbol(&conn, &sym.id, None)?;
    let type_refs: Vec<_> = all_edges
        .iter()
        .filter(|(e, _)| {
            e.edge_type == symbols::EdgeType::TypeRef
                || e.edge_type == symbols::EdgeType::Implements
        })
        .collect();

    if !type_refs.is_empty() {
        println!("{} ({}):", "Type References".bold(), type_refs.len());
        for (edge, related) in &type_refs {
            if let Some(s) = related {
                println!(
                    "  {} {} ({}:{})",
                    edge.edge_type.as_str().dimmed(),
                    s.name.cyan(),
                    s.file_path,
                    s.line_start
                );
            }
        }
        println!();
    }

    let importers = graph::find_importers(&conn, &sym.id, 20)?;
    if !importers.is_empty() {
        println!("{} ({}):", "Imported by".bold(), importers.len());
        for i in &importers {
            println!("  ⊂ {} ({})", i.name.cyan(), i.file_path);
        }
    }

    Ok(0)
}

fn run_relations(
    name: &str,
    query_type: &str,
    depth: u32,
    limit: usize,
    repo_id: Option<&str>,
) -> Result<i32> {
    let conn = db::open()?;

    let sym = match resolve_symbol(&conn, name, repo_id)? {
        Some(s) => s,
        None => {
            println!("Symbol {:?} not found", name);
            return Ok(1);
        }
    };

    let results = graph::analyze_relationships(&conn, &sym.id, query_type, depth, limit)?;

    if results.is_empty() {
        println!("No {} relationships found for {:?}", query_type, name);
        return Ok(0);
    }

    println!(
        "{} {} for {} (depth {}):",
        results.len().to_string().bold(),
        query_type,
        name.bold(),
        depth
    );
    println!();

    for node in &results {
        println!(
            "  {} {} {} ({}:{})",
            format!("d{}", node.depth).dimmed(),
            node.edge_type.dimmed(),
            node.symbol.name.cyan(),
            node.symbol.file_path,
            node.symbol.line_start
        );
    }

    Ok(0)
}

/// Bring the SQLite projection up to date before a blast-radius query.
///
/// `impact` reads `symbols` and `edges`, which only change when the index runs.
/// The graph-backed commands refresh themselves, so without this an agent that
/// edits a file sees the edit in `ask` and misses it in `impact` — and `impact`
/// is the one being asked "is this safe to change".
///
/// A failure is not fatal: the previous projection is stale, not wrong, and an
/// unindexed repository has nothing to refresh.
fn refresh_impact_projection(repo_id: Option<&str>) {
    let Ok(root) = std::env::current_dir() else {
        return;
    };
    if !crate::graph::store::GraphPaths::new(&root).exists() {
        return;
    }

    let resolved = repo_id.map(str::to_string).unwrap_or_else(|| {
        root.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string()
    });

    let Ok(conn) = db::open() else {
        return;
    };
    let path = root.to_string_lossy();
    let _ = indexer::index_directory(&conn, &path, &resolved, "main", true);
}

fn run_impact(
    name: &str,
    direction: &str,
    depth: u32,
    limit: usize,
    repo_id: Option<&str>,
) -> Result<i32> {
    refresh_impact_projection(repo_id);

    let conn = db::open()?;

    let sym = match resolve_symbol(&conn, name, repo_id)? {
        Some(s) => s,
        None => {
            println!("Symbol {:?} not found", name);
            return Ok(1);
        }
    };

    let dir = graph::Direction::from_str(direction).unwrap_or(graph::Direction::Both);
    let results = graph::bfs_impact(&conn, &sym.id, dir, depth, limit, None)?;

    if results.is_empty() {
        println!(
            "No impact detected for {:?} (direction: {})",
            name, direction
        );
        return Ok(0);
    }

    println!(
        "{} {} symbols affected by {} (direction: {}, depth: {}):",
        "⚡".yellow(),
        results.len().to_string().bold(),
        name.bold(),
        direction,
        depth
    );
    println!();

    for node in &results {
        let indent = "  ".repeat(node.depth as usize);
        println!(
            "{}{}→ {} [{}] ({}:{})",
            indent,
            node.edge_type.dimmed(),
            node.symbol.name.cyan(),
            node.symbol.kind,
            node.symbol.file_path,
            node.symbol.line_start
        );
    }

    Ok(0)
}

fn run_repos() -> Result<i32> {
    let conn = db::open()?;
    let repos = db::list_repositories(&conn)?;

    if repos.is_empty() {
        println!("No indexed repositories. Run `tok mem index <path>` to get started.");
        return Ok(0);
    }

    println!("{} indexed repositories:", repos.len().to_string().bold());
    println!();

    for repo in &repos {
        println!(
            "  {} {} — {} symbols, {} edges, {} files",
            repo.repo_id.bold(),
            repo.path.dimmed(),
            repo.symbol_count,
            repo.edge_count,
            repo.file_count,
        );
        if !repo.last_indexed_at.is_empty() {
            println!("    last indexed: {}", repo.last_indexed_at.dimmed());
        }
    }

    Ok(0)
}

fn run_status(repo_id: Option<&str>) -> Result<i32> {
    let conn = db::open()?;

    let resolved_id = match repo_id {
        Some(id) => id.to_string(),
        None => std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "unknown".to_string()),
    };

    match db::get_repository(&conn, &resolved_id)? {
        Some(repo) => {
            println!("{}", "Repository Status".bold().underline());
            println!("  ID:           {}", repo.repo_id.bold());
            println!("  Path:         {}", repo.path);
            println!("  Branch:       {}", repo.branch);
            println!("  Last indexed: {}", repo.last_indexed_at);
            println!("  Symbols:      {}", repo.symbol_count);
            println!("  Edges:        {}", repo.edge_count);
            println!("  Files:        {}", repo.file_count);

            let (_sym_count, _edge_count, _file_count) = db::repo_stats(&conn, &resolved_id)?;

            let kind_breakdown = kind_counts(&conn, &resolved_id)?;
            if !kind_breakdown.is_empty() {
                println!();
                println!("{}", "Symbol Breakdown".bold());
                for (kind, count) in &kind_breakdown {
                    println!("  {:12} {}", kind, count);
                }
            }
        }
        None => {
            println!(
                "Repository {:?} not found. Run `tok mem index` first.",
                resolved_id
            );
            return Ok(1);
        }
    }

    Ok(0)
}

fn run_forget(repo_id: &str) -> Result<i32> {
    let conn = db::open()?;

    match db::get_repository(&conn, repo_id)? {
        Some(_) => {
            db::clear_repo_symbols(&conn, repo_id)?;
            db::delete_repository(&conn, repo_id)?;
            println!("{} Removed {} from memory", "✓".green(), repo_id.bold());
            Ok(0)
        }
        None => {
            println!("Repository {:?} not found", repo_id);
            Ok(1)
        }
    }
}

fn run_evolution(
    from: &str,
    to: &str,
    mode: &str,
    repo_id: Option<&str>,
    max_symbols: usize,
) -> Result<i32> {
    let conn = db::open()?;

    let resolved_id = resolve_repo_id(repo_id);

    // Check if repo exists
    if db::get_repository(&conn, &resolved_id)?.is_none() {
        println!("Repository {:?} not found", resolved_id);
        return Ok(1);
    }

    // Populate episodes from git log
    let repo = db::get_repository(&conn, &resolved_id)?.unwrap();
    let ep_count = evolution::populate_episodes(
        &conn,
        &resolved_id,
        &repo.path,
        &repo.branch,
        Some(from),
        Some(to),
    )?;

    let scoring =
        evolution::ScoringMode::from_str(mode).unwrap_or(evolution::ScoringMode::Compound);
    let entries = evolution::query_evolution(&conn, &resolved_id, from, to, scoring, max_symbols)?;

    if entries.is_empty() {
        println!(
            "No changes detected between {} and {} (populated {} episodes)",
            from, to, ep_count
        );
        return Ok(0);
    }

    println!(
        "{} changes ({} mode, {} episodes) from {} to {}:",
        entries.len().to_string().bold(),
        mode,
        ep_count,
        from,
        to,
    );
    println!();

    for entry in &entries {
        println!(
            "  {} [{}] {} ({}:{}) score={:.1}",
            entry.change_type.dimmed(),
            entry.symbol_kind.dimmed(),
            entry.symbol_name.cyan(),
            entry.file_path,
            entry.change_count,
            entry.score,
        );
    }

    Ok(0)
}

fn run_timeline(name: &str, repo_id: Option<&str>) -> Result<i32> {
    let conn = db::open()?;

    let sym = match resolve_symbol(&conn, name, repo_id)? {
        Some(s) => s,
        None => {
            println!("Symbol {:?} not found", name);
            return Ok(1);
        }
    };

    // Populate episodes first
    let resolved_id = resolve_repo_id(repo_id);
    if let Some(repo) = db::get_repository(&conn, &resolved_id)? {
        let _ =
            evolution::populate_episodes(&conn, &resolved_id, &repo.path, &repo.branch, None, None);
    }

    let entries = evolution::get_timeline(&conn, &sym.id)?;

    if entries.is_empty() {
        println!("No change history for {:?}", name);
        return Ok(0);
    }

    println!(
        "{} for {} ({} events):",
        "Timeline".bold().underline(),
        name.cyan().bold(),
        entries.len()
    );
    println!();

    for entry in &entries {
        println!(
            "  {} {} {} {}",
            entry.timestamp.dimmed(),
            entry.change_type,
            entry.commit_hash[..8.min(entry.commit_hash.len())].dimmed(),
            entry.diff_summary,
        );
    }

    Ok(0)
}

fn run_changes(
    since_episode: Option<&str>,
    since_time: Option<&str>,
    repo_id: Option<&str>,
) -> Result<i32> {
    let conn = db::open()?;
    let resolved_id = resolve_repo_id(repo_id);

    if db::get_repository(&conn, &resolved_id)?.is_none() {
        println!("Repository {:?} not found", resolved_id);
        return Ok(1);
    }

    // Populate latest episodes
    let repo = db::get_repository(&conn, &resolved_id)?.unwrap();
    let _ = evolution::populate_episodes(&conn, &resolved_id, &repo.path, &repo.branch, None, None);

    let (entries, anchor) =
        evolution::get_changes_since(&conn, &resolved_id, since_episode, since_time)?;

    if entries.is_empty() {
        println!("No changes since last anchor");
    } else {
        println!(
            "{} symbols changed since last session:",
            entries.len().to_string().bold()
        );
        println!();

        for entry in &entries {
            println!(
                "  {} [{}] {} ({})",
                entry.change_type.dimmed(),
                entry.symbol_kind.dimmed(),
                entry.symbol_name.cyan(),
                entry.file_path,
            );
        }
    }

    println!();
    println!("{}", "Session Anchor (save for next session):".bold());
    println!("  episode: {}", anchor.last_episode_id);
    println!("  time:    {}", anchor.last_reference_time);

    Ok(0)
}

fn run_detect(files: &[String], repo_id: Option<&str>) -> Result<i32> {
    let conn = db::open()?;
    let resolved_id = resolve_repo_id(repo_id);

    let entries = evolution::detect_changes(&conn, &resolved_id, files)?;

    if entries.is_empty() {
        println!("No indexed symbols affected by the specified files");
        return Ok(0);
    }

    println!(
        "{} symbols affected across {} files:",
        entries.len().to_string().bold(),
        files.len()
    );
    println!();

    for entry in &entries {
        println!(
            "  [{}] {} ({})",
            entry.symbol_kind.dimmed(),
            entry.symbol_name.cyan(),
            entry.file_path,
        );
    }

    Ok(0)
}

fn run_central(repo_id: Option<&str>, limit: usize) -> Result<i32> {
    let conn = db::open()?;
    let resolved_id = resolve_repo_id(repo_id);

    let results = quality::find_central_symbols(&conn, &resolved_id, limit)?;

    if results.is_empty() {
        println!("No symbols with edges found. Index a codebase first.");
        return Ok(0);
    }

    println!(
        "{} most central symbols in {}:",
        results.len().to_string().bold(),
        resolved_id.bold()
    );
    println!();

    for sym in &results {
        println!(
            "  {} [{}] in={} out={} score={:.0}  ({})",
            sym.name.cyan().bold(),
            sym.kind.dimmed(),
            sym.in_degree,
            sym.out_degree,
            sym.score,
            sym.file_path,
        );
    }

    Ok(0)
}

fn run_bridges(repo_id: Option<&str>, limit: usize) -> Result<i32> {
    let conn = db::open()?;
    let resolved_id = resolve_repo_id(repo_id);

    let results = quality::find_bridge_symbols(&conn, &resolved_id, limit)?;

    if results.is_empty() {
        println!("No bridge symbols detected");
        return Ok(0);
    }

    println!(
        "{} bridge symbols in {}:",
        results.len().to_string().bold(),
        resolved_id.bold()
    );
    println!();

    for sym in &results {
        println!(
            "  {} [{}] connects {} communities ({})",
            sym.name.cyan().bold(),
            sym.kind.dimmed(),
            sym.communities_connected,
            sym.file_path,
        );
    }

    Ok(0)
}

fn run_communities(repo_id: Option<&str>, min_size: usize, limit: usize) -> Result<i32> {
    let conn = db::open()?;
    let resolved_id = resolve_repo_id(repo_id);

    let results = quality::detect_communities(&conn, &resolved_id, min_size, limit)?;

    if results.is_empty() {
        println!("No communities detected (min size: {})", min_size);
        return Ok(0);
    }

    println!(
        "{} communities in {}:",
        results.len().to_string().bold(),
        resolved_id.bold()
    );
    println!();

    for comm in &results {
        println!(
            "  Community {} ({} members): {}",
            comm.id,
            comm.member_count,
            comm.label.cyan(),
        );
    }

    Ok(0)
}

fn run_dead_code(repo_id: Option<&str>, include_tests: bool, limit: usize) -> Result<i32> {
    let conn = db::open()?;
    let resolved_id = resolve_repo_id(repo_id);

    let results = quality::find_dead_code(
        &conn,
        &resolved_id,
        include_tests,
        limit,
        Some(&["Function", "Method", "Class", "Struct"]),
    )?;

    if results.is_empty() {
        println!("No unreferenced symbols found");
        return Ok(0);
    }

    println!(
        "{} symbols with zero inbound references:",
        results.len().to_string().bold()
    );
    println!();

    for entry in &results {
        println!(
            "  [{}] {} ({}:{})",
            entry.kind.dimmed(),
            entry.name.yellow(),
            entry.file_path,
            entry.line_start,
        );
    }

    Ok(0)
}

fn run_complexity(repo_id: Option<&str>, limit: usize, min_complexity: u32) -> Result<i32> {
    let conn = db::open()?;
    let resolved_id = resolve_repo_id(repo_id);

    let results = quality::estimate_complexity(&conn, &resolved_id, limit, min_complexity)?;

    if results.is_empty() {
        println!(
            "No functions above complexity threshold ({})",
            min_complexity
        );
        return Ok(0);
    }

    println!(
        "{} most complex functions (threshold {}):",
        results.len().to_string().bold(),
        min_complexity
    );
    println!();

    for entry in &results {
        let bar = "█".repeat((entry.complexity as usize).min(40));
        println!(
            "  {:>3} {} [{}] {}:{}",
            entry.complexity,
            bar.red(),
            entry.kind.dimmed(),
            entry.name.cyan(),
            entry.file_path,
        );
    }

    Ok(0)
}

fn resolve_repo_id(repo_id: Option<&str>) -> String {
    match repo_id {
        Some(id) => id.to_string(),
        None => std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "unknown".to_string()),
    }
}

/// Resolve a symbol by exact name, falling back to fuzzy substring match.
fn resolve_symbol(
    conn: &rusqlite::Connection,
    name: &str,
    repo_id: Option<&str>,
) -> Result<Option<symbols::Symbol>> {
    let exact = db::find_symbol_by_name(conn, name, repo_id, None, 1)?;
    if let Some(s) = exact.into_iter().next() {
        return Ok(Some(s));
    }
    let fuzzy = db::find_symbol_fuzzy(conn, name, repo_id, 1)?;
    Ok(fuzzy.into_iter().next())
}

// ── Display helpers ──

fn print_symbol_line(sym: &symbols::Symbol) {
    println!(
        "  {} {} {}:{}",
        format!("[{}]", sym.kind).dimmed(),
        sym.name.cyan().bold(),
        sym.file_path,
        sym.line_start,
    );
    if !sym.signature.is_empty() {
        println!("    {}", sym.signature.dimmed());
    }
}

fn print_symbol_detail(sym: &symbols::Symbol) {
    println!("  Name:      {}", sym.name.cyan().bold());
    println!("  Kind:      {}", sym.kind);
    println!("  File:      {}:{}", sym.file_path, sym.line_start);
    if !sym.signature.is_empty() {
        println!("  Signature: {}", sym.signature);
    }
    if !sym.doc_comment.is_empty() {
        println!("  Doc:       {}", sym.doc_comment);
    }
    println!("  Repo:      {}", sym.repo_id);
    println!("  ID:        {}", sym.id.dimmed());
}

fn kind_counts(conn: &rusqlite::Connection, repo_id: &str) -> Result<Vec<(String, usize)>> {
    let mut stmt = conn.prepare(
        "SELECT kind, COUNT(*) FROM symbols WHERE repo_id = ?1 GROUP BY kind ORDER BY COUNT(*) DESC",
    )?;

    let rows = stmt
        .query_map(rusqlite::params![repo_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    Ok(rows)
}
