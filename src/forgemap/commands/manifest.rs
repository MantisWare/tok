//! `tok forgemap manifest` — generate the `.forgemap` project manifest.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use colored::Colorize;

use crate::forgemap::collect::{collect_files, CollectOptions};
use crate::forgemap::constants::MANIFEST_FILENAME;
use crate::forgemap::graph::{build_used_by, detect_packages};
use crate::forgemap::manifest_io::{
    detect_project_meta, read_existing_manifest, write_manifest, WriteManifestOpts,
};
use crate::forgemap::scan::scan_file;
use crate::forgemap::types::{ManifestOptions, PackageManifestEntry};

/// Run the manifest generation pipeline.
pub fn run_manifest(opts: &ManifestOptions) -> Result<String> {
    let manifest_path = opts.repo_root.join(MANIFEST_FILENAME);

    // Read existing manifest for preserved fields.
    let existing = if manifest_path.exists() {
        read_existing_manifest(&manifest_path).unwrap_or_default()
    } else {
        Default::default()
    };

    let collect_opts = CollectOptions {
        target: opts.target.clone(),
        repo_root: opts.repo_root.clone(),
        extensions: opts.extensions.clone(),
        exclude: opts.exclude.clone(),
    };

    let files = collect_files(&collect_opts).context("Failed to collect source files")?;

    if opts.verbose {
        eprintln!(
            "{} Collected {} source files for manifest",
            "→".cyan(),
            files.len()
        );
    }

    // Scan.
    let mut infos = BTreeMap::new();
    for path in &files {
        let info = scan_file(path, &opts.repo_root);
        infos.insert(info.rel.clone(), info);
    }

    // Build graph.
    let used_by = build_used_by(&infos);

    // Detect packages.
    let packages = detect_packages(&infos, &used_by);

    // Detect project meta.
    let (detected_name, detected_desc) = detect_project_meta(&opts.repo_root);

    let project = if existing.project.is_empty() {
        detected_name
    } else {
        existing.project.clone()
    };

    let description = if existing.description.is_empty() {
        detected_desc
    } else {
        existing.description.clone()
    };

    let mode = if existing.mode.is_empty() {
        "semi"
    } else {
        &existing.mode
    };

    // Build package manifest entries.
    let mut pkg_entries: BTreeMap<String, PackageManifestEntry> = BTreeMap::new();
    for pkg in &packages {
        pkg_entries.insert(
            pkg.key.clone(),
            PackageManifestEntry {
                purpose: pkg.purpose.clone(),
                key_files: pkg.key_files.clone(),
                depends_on: pkg.depends_on.clone(),
            },
        );
    }

    let write_opts = WriteManifestOpts {
        abs_path: &manifest_path,
        project: &project,
        description: &description,
        mode,
        packages: &pkg_entries,
        cross_cutting_block: &existing.cross_cutting_block,
        agent_sessions_block: &existing.agent_sessions_block,
        dry_run: opts.dry_run,
    };

    write_manifest(&write_opts)
}
