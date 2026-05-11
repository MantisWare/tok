//! `tok forgemap init` — first-time annotation pass.
//!
//! Pipeline: collect files → scan each → build used_by → build header → inject.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use colored::Colorize;

use crate::forgemap::collect::{collect_files, CollectOptions};
use crate::forgemap::fmt::{file_purpose_heuristic, gen_session_id};
use crate::forgemap::graph::build_used_by;
use crate::forgemap::header::{build_header, BuildHeaderOpts};
use crate::forgemap::inject::{inject_header, replace_header, safe_write_file};
use crate::forgemap::scan::scan_file;
use crate::forgemap::types::{InitOptions, InitResult};

/// Run the init pipeline: scan all files, build graph, inject headers.
pub fn run_init(opts: &InitOptions) -> Result<InitResult> {
    let collect_opts = CollectOptions {
        target: opts.target.clone(),
        repo_root: opts.repo_root.clone(),
        extensions: opts.extensions.clone(),
        exclude: opts.exclude.clone(),
    };

    let files = collect_files(&collect_opts).context("Failed to collect source files")?;

    if opts.verbose {
        eprintln!("{} Collected {} source files", "→".cyan(), files.len());
    }

    // Phase 1: Scan all files.
    let mut infos = BTreeMap::new();
    for path in &files {
        let info = scan_file(path, &opts.repo_root);
        infos.insert(info.rel.clone(), info);
    }

    // Phase 2: Build reverse dependency graph.
    let used_by = build_used_by(&infos);

    // Phase 3: Inject headers.
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let session_id = if opts.session_id.is_empty() {
        gen_session_id()
    } else {
        opts.session_id.clone()
    };

    let mut result = InitResult {
        total_files: files.len(),
        ..Default::default()
    };

    let sorted_rels: Vec<String> = infos.keys().cloned().collect();

    for rel in &sorted_rels {
        let info = match infos.get(rel) {
            Some(i) => i,
            None => continue,
        };

        // Skip already-annotated files unless --force.
        if info.has_forgemap && !opts.force {
            result.skipped += 1;
            if opts.verbose {
                eprintln!("  {} {} (already annotated)", "skip".dimmed(), rel);
            }
            continue;
        }

        let file_used_by = used_by.get(rel).cloned().unwrap_or_default();

        let purpose = file_purpose_heuristic(rel);
        let rules_str = info
            .header
            .as_ref()
            .map(|h| h.rules.as_str())
            .unwrap_or("none");

        let header_opts = BuildHeaderOpts {
            rel,
            purpose: &purpose,
            exports: &info.exports,
            used_by: &file_used_by,
            related: None,
            wiki: None,
            rules: rules_str,
            model_id: &opts.model_id,
            today: &today,
            session_id: &session_id,
        };

        let header = build_header(&header_opts);

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

        let new_source = if opts.force {
            replace_header(&source, &header, ext)
        } else {
            inject_header(&source, &header, ext)
        };

        if !opts.dry_run {
            if let Err(e) = safe_write_file(&info.abs_path, &new_source) {
                if opts.verbose {
                    eprintln!("  {} {} — write failed: {}", "ERR".red(), rel, e);
                }
                result.errors += 1;
                continue;
            }
        }

        result.annotated += 1;
        if opts.verbose {
            eprintln!("  {} {}", "✓".green(), rel);
        }
    }

    Ok(result)
}

/// Format the init result for display.
pub fn format_init_result(result: &InitResult, target: &str, dry_run: bool) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "ForgeMap Init{}\n",
        if dry_run { " (dry run)" } else { "" }
    ));
    out.push_str(&format!("Target      {}\n", target));
    out.push_str(&format!("Files       {}\n", result.total_files));
    out.push('\n');
    out.push_str(&format!(
        "Annotated   {}/{}\n",
        result.annotated, result.total_files
    ));
    out.push_str(&format!("Skipped     {}\n", result.skipped));
    if result.errors > 0 {
        out.push_str(&format!("Errors      {}\n", result.errors));
    }
    out
}
