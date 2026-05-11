//! `tok forgemap check` — coverage report. Exits 1 if any files are unannotated.

use anyhow::{Context, Result};

use crate::forgemap::collect::{collect_files, CollectOptions};
use crate::forgemap::scan::scan_file;
use crate::forgemap::types::CheckResult;

/// Options for the check command.
pub struct CheckOptions {
    pub target: std::path::PathBuf,
    pub repo_root: std::path::PathBuf,
    pub extensions: Option<Vec<String>>,
    pub exclude: Vec<String>,
    #[allow(dead_code)]
    pub verbose: bool,
}

/// Run coverage check: scan files and report annotation status.
pub fn run_check(opts: &CheckOptions) -> Result<CheckResult> {
    let collect_opts = CollectOptions {
        target: opts.target.clone(),
        repo_root: opts.repo_root.clone(),
        extensions: opts.extensions.clone(),
        exclude: opts.exclude.clone(),
    };

    let files = collect_files(&collect_opts).context("Failed to collect source files")?;

    let mut result = CheckResult {
        total_files: files.len(),
        ..Default::default()
    };

    for path in &files {
        let info = scan_file(path, &opts.repo_root);

        if !info.parseable {
            result.unparseable.push(info.rel.clone());
            continue;
        }

        if info.has_forgemap {
            result.annotated += 1;
        } else {
            result.missing.push(info.rel.clone());
        }
    }

    result.all_annotated = result.missing.is_empty();
    Ok(result)
}

/// Format the check result for display.
pub fn format_check_result(result: &CheckResult, target: &str, verbose: bool) -> String {
    let mut out = String::new();
    out.push_str("ForgeMap Check\n");
    out.push_str(&format!("Target      {}\n", target));
    out.push_str(&format!("Files       {}\n", result.total_files));
    out.push('\n');

    let pct = if result.total_files > 0 {
        (result.annotated as f64 / result.total_files as f64 * 100.0) as u32
    } else {
        0
    };

    out.push_str(&format!(
        "L1 (module headers)    {}/{}  ({}%)\n",
        result.annotated, result.total_files, pct
    ));

    if !result.unparseable.is_empty() {
        out.push_str(&format!(
            "Unparseable            {}\n",
            result.unparseable.len()
        ));
    }

    if verbose && !result.missing.is_empty() {
        out.push_str("\nMissing L1:\n");
        for f in &result.missing {
            out.push_str(&format!("  {}\n", f));
        }
    }

    out.push('\n');
    if result.all_annotated {
        out.push_str("OK — fully annotated\n");
    } else {
        out.push_str(&format!(
            "INCOMPLETE — {} files missing headers\n",
            result.missing.len()
        ));
    }

    out
}
