//! Dependency graph construction — reverse dependency map, package detection, and ranking.
//!
//! Implements FORGEMAP.md §8: `build_used_by`, `detect_packages`, `depends_on`.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;

use super::constants::{PACKAGE_DEPTH, SKIP_DIRS};
use super::fmt::package_purpose_heuristic;
use super::types::{FileInfo, PackageInfo, RelPath, UsedByMap};

/// Build the reverse dependency map: for each file, who imports it?
///
/// Only files present in `infos` appear as keys. Dependencies pointing to
/// files outside the scanned set are dropped.
pub fn build_used_by(infos: &BTreeMap<RelPath, FileInfo>) -> UsedByMap {
    let mut ub: UsedByMap = BTreeMap::new();

    for (importer, info) in infos {
        for (dep, syms) in &info.deps {
            if infos.contains_key(dep) {
                ub.entry(dep.clone())
                    .or_default()
                    .entry(importer.clone())
                    .or_default()
                    .extend(syms.iter().cloned());
            }
        }
    }

    // Deduplicate and sort symbol lists.
    for importers in ub.values_mut() {
        for syms in importers.values_mut() {
            syms.sort();
            syms.dedup();
        }
    }

    ub
}

/// Detect packages (directory subtrees) from the file set.
///
/// A "package" is any directory subtree capped at `PACKAGE_DEPTH` path segments.
/// Files at the repo root go under the empty-string key `""`.
pub fn detect_packages(
    infos: &BTreeMap<RelPath, FileInfo>,
    used_by: &UsedByMap,
) -> Vec<PackageInfo> {
    let skip_dirs: HashSet<&str> = SKIP_DIRS.iter().copied().collect();

    // Map: package key -> list of files.
    let mut pkg_files: BTreeMap<String, Vec<RelPath>> = BTreeMap::new();

    for rel in infos.keys() {
        let pkg_key = package_key_for(rel, &skip_dirs);
        pkg_files.entry(pkg_key).or_default().push(rel.clone());
    }

    let mut packages = Vec::new();

    for (key, files) in &pkg_files {
        let key_files = rank_key_files(files, infos, used_by);
        let depends_on = compute_depends_on(key, files, infos, &pkg_files);
        let purpose = package_purpose_heuristic(key, files);

        packages.push(PackageInfo {
            key: key.clone(),
            files: files.clone(),
            purpose,
            key_files,
            depends_on,
        });
    }

    packages
}

/// Determine the package key for a file, capped at `PACKAGE_DEPTH` segments.
fn package_key_for(rel: &str, skip_dirs: &HashSet<&str>) -> String {
    let path = Path::new(rel);
    let parent = match path.parent() {
        Some(p) => p,
        None => return String::new(),
    };

    let components: Vec<&str> = parent
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();

    if components.is_empty() {
        return String::new();
    }

    // Check for skip dirs in the path.
    for &comp in &components {
        if skip_dirs.contains(comp) {
            return String::new();
        }
    }

    let depth = components.len().min(PACKAGE_DEPTH);
    components[..depth].join("/")
}

/// Rank files by `importer_count * 10 + export_count`, return top 5 basenames.
fn rank_key_files(
    files: &[RelPath],
    infos: &BTreeMap<RelPath, FileInfo>,
    used_by: &UsedByMap,
) -> Vec<String> {
    let mut scored: Vec<(String, usize)> = files
        .iter()
        .filter_map(|rel| {
            let info = infos.get(rel)?;
            let basename = Path::new(rel).file_name()?.to_str()?.to_string();
            let importer_count = used_by.get(rel).map(|m| m.len()).unwrap_or(0);
            let export_count = info.exports.len();
            let score = importer_count * 10 + export_count;
            Some((basename, score))
        })
        .collect();

    scored.sort_by(|a, b| b.1.cmp(&a.1));
    scored.dedup_by(|a, b| a.0 == b.0);
    scored.into_iter().take(5).map(|(name, _)| name).collect()
}

/// Compute `depends_on` for a package: other packages whose files are imported
/// by files in this package. Self-deps excluded.
fn compute_depends_on(
    pkg_key: &str,
    files: &[RelPath],
    infos: &BTreeMap<RelPath, FileInfo>,
    all_pkg_files: &BTreeMap<String, Vec<RelPath>>,
) -> Vec<String> {
    let skip_dirs: HashSet<&str> = SKIP_DIRS.iter().copied().collect();
    let mut dep_keys: BTreeSet<String> = BTreeSet::new();

    for rel in files {
        if let Some(info) = infos.get(rel) {
            for dep_rel in info.deps.keys() {
                let dep_pkg = package_key_for(dep_rel, &skip_dirs);
                if dep_pkg != pkg_key && all_pkg_files.contains_key(&dep_pkg) {
                    let suffixed = if dep_pkg.is_empty() {
                        "/".to_string()
                    } else {
                        format!("{}/", dep_pkg)
                    };
                    dep_keys.insert(suffixed);
                }
            }
        }
    }

    dep_keys.into_iter().collect()
}
