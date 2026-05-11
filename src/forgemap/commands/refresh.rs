//! `tok forgemap refresh` — structural-only update of `exports:` and `used_by:`.
//!
//! Re-scans all files, recomputes the dependency graph, and updates only the
//! structural fields. Never touches `rules:`, `agent:`, `message:`.
//! Files without an existing header are skipped.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use colored::Colorize;

use crate::forgemap::collect::{collect_files, CollectOptions};
use crate::forgemap::graph::build_used_by;
use crate::forgemap::inject::{refresh_header, safe_write_file};
use crate::forgemap::scan::scan_file;
use crate::forgemap::types::RefreshResult;

/// Options for the refresh command.
pub struct RefreshOptions {
    pub target: std::path::PathBuf,
    pub repo_root: std::path::PathBuf,
    pub extensions: Option<Vec<String>>,
    pub exclude: Vec<String>,
    pub dry_run: bool,
    pub verbose: bool,
}

/// Run structural-only refresh on annotated files.
pub fn run_refresh(opts: &RefreshOptions) -> Result<RefreshResult> {
    let collect_opts = CollectOptions {
        target: opts.target.clone(),
        repo_root: opts.repo_root.clone(),
        extensions: opts.extensions.clone(),
        exclude: opts.exclude.clone(),
    };

    let files = collect_files(&collect_opts).context("Failed to collect source files")?;

    if opts.verbose {
        eprintln!(
            "{} Collected {} source files for refresh",
            "→".cyan(),
            files.len()
        );
    }

    // Phase 1: Scan.
    let mut infos = BTreeMap::new();
    for path in &files {
        let info = scan_file(path, &opts.repo_root);
        infos.insert(info.rel.clone(), info);
    }

    // Phase 2: Rebuild used_by.
    let used_by = build_used_by(&infos);

    // Phase 3: Refresh headers.
    let mut result = RefreshResult {
        total_files: files.len(),
        ..Default::default()
    };

    let sorted_rels: Vec<String> = infos.keys().cloned().collect();

    for rel in &sorted_rels {
        let info = match infos.get(rel) {
            Some(i) => i,
            None => continue,
        };

        if !info.has_forgemap {
            result.skipped_no_header += 1;
            if opts.verbose {
                eprintln!("  {} {} (no header)", "skip".dimmed(), rel);
            }
            continue;
        }

        let source = match std::fs::read_to_string(&info.abs_path) {
            Ok(s) => s,
            Err(e) => {
                if opts.verbose {
                    eprintln!("  {} {} — {}", "ERR".red(), rel, e);
                }
                result.errors += 1;
                continue;
            }
        };

        let ext = info
            .abs_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        let file_used_by = used_by.get(rel).cloned().unwrap_or_default();

        let refresh = refresh_header(&source, &info.exports, &file_used_by, ext);

        if !refresh.changed {
            result.unchanged += 1;
            if opts.verbose {
                eprintln!("  {} {} (unchanged)", "—".dimmed(), rel);
            }
            continue;
        }

        if !opts.dry_run {
            if let Err(e) = safe_write_file(&info.abs_path, &refresh.source) {
                if opts.verbose {
                    eprintln!("  {} {} — write failed: {}", "ERR".red(), rel, e);
                }
                result.errors += 1;
                continue;
            }
        }

        result.updated += 1;
        if opts.verbose {
            eprintln!(
                "  {} {} ({})",
                "✓".green(),
                rel,
                refresh.changed_fields.join(", ")
            );
        }
    }

    Ok(result)
}

/// Format the refresh result for display.
pub fn format_refresh_result(result: &RefreshResult, target: &str, dry_run: bool) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "ForgeMap Refresh{}\n",
        if dry_run { " (dry run)" } else { "" }
    ));
    out.push_str(&format!("Target      {}\n", target));
    out.push_str(&format!("Files       {}\n", result.total_files));
    out.push('\n');
    out.push_str(&format!("Updated     {}\n", result.updated));
    out.push_str(&format!("Unchanged   {}\n", result.unchanged));
    out.push_str(&format!("No header   {}\n", result.skipped_no_header));
    if result.errors > 0 {
        out.push_str(&format!("Errors      {}\n", result.errors));
    }
    out
}
