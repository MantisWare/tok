//! `tok forgemap wiki bootstrap` and `tok forgemap wiki sync` command handlers.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use colored::Colorize;

use crate::forgemap::collect::{collect_files, CollectOptions};
use crate::forgemap::scan::scan_file;
use crate::forgemap::types::WikiBootstrapOptions;
use crate::forgemap::wiki;

/// Run wiki bootstrap: emit per-file Obsidian vault.
pub fn run_wiki_bootstrap(opts: &WikiBootstrapOptions) -> Result<(usize, usize)> {
    let collect_opts = CollectOptions {
        target: opts.target.clone(),
        repo_root: opts.repo_root.clone(),
        extensions: opts.extensions.clone(),
        exclude: opts.exclude.clone(),
    };

    let files = collect_files(&collect_opts).context("Failed to collect source files")?;

    if opts.verbose {
        eprintln!(
            "{} Collected {} source files for wiki",
            "→".cyan(),
            files.len()
        );
    }

    let mut infos = BTreeMap::new();
    for path in &files {
        let info = scan_file(path, &opts.repo_root);
        infos.insert(info.rel.clone(), info);
    }

    wiki::bootstrap_wiki(&infos, &opts.out_dir, &opts.repo_root)
}

/// Run wiki sync: regenerate narrative project wiki.
pub fn run_wiki_sync(repo_root: &Path, out_path: &Path, verbose: bool) -> Result<()> {
    if verbose {
        eprintln!(
            "{} Syncing project wiki to {}",
            "→".cyan(),
            out_path.display()
        );
    }
    wiki::sync_wiki(repo_root, out_path)
}
