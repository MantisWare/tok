//! File collection — walks a project tree, filters by extension, skips vendor/build dirs.
//!
//! Reuses the `ignore` crate (same approach as `mem::indexer`) to respect `.gitignore`
//! rules and skip binary/vendor directories.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::constants::{SKIP_DIRS, SUPPORTED_EXTENSIONS, TEST_SUFFIXES};

/// Options controlling file collection.
pub struct CollectOptions {
    /// Absolute path — file or directory.
    pub target: PathBuf,
    /// Absolute path to the repo root (used by callers for relative path computation).
    #[allow(dead_code)]
    pub repo_root: PathBuf,
    /// File extensions to include (default: `SUPPORTED_EXTENSIONS`).
    pub extensions: Option<Vec<String>>,
    /// Glob patterns to exclude (matched against basename and relative-to-target path).
    pub exclude: Vec<String>,
}

/// Collect source files under `target`, returning sorted absolute paths.
pub fn collect_files(opts: &CollectOptions) -> Result<Vec<PathBuf>> {
    let target = opts
        .target
        .canonicalize()
        .with_context(|| format!("Cannot resolve target: {}", opts.target.display()))?;

    let allowed_exts: HashSet<&str> = match &opts.extensions {
        Some(exts) => exts.iter().map(|s| s.as_str()).collect(),
        None => SUPPORTED_EXTENSIONS.iter().copied().collect(),
    };

    let skip_dirs: HashSet<&str> = SKIP_DIRS.iter().copied().collect();

    // If target is a single file, check it and return.
    if target.is_file() {
        if is_supported_file(&target, &allowed_exts)
            && !is_excluded(&target, &opts.target, &opts.exclude)
        {
            return Ok(vec![target]);
        }
        return Ok(Vec::new());
    }

    let target_is_explicit_dir = target.is_dir();
    let mut results = Vec::new();

    let walker = ignore::WalkBuilder::new(&target)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .filter_entry(move |entry| {
            let name = entry.file_name().to_string_lossy();
            !skip_dirs.contains(name.as_ref())
        })
        .build();

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        if !is_supported_file(path, &allowed_exts) {
            continue;
        }

        if is_excluded(path, &opts.target, &opts.exclude) {
            continue;
        }

        // Test file exclusion: skip test files unless the target is their direct parent.
        if (!target_is_explicit_dir || !is_direct_child(&target, path)) && is_test_file(path) {
            continue;
        }

        results.push(path.to_path_buf());
    }

    results.sort();
    Ok(results)
}

fn is_supported_file(path: &Path, allowed_exts: &HashSet<&str>) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| allowed_exts.contains(e))
        .unwrap_or(false)
}

fn is_test_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    TEST_SUFFIXES.iter().any(|suffix| {
        if suffix.starts_with("test_") {
            name.starts_with(suffix)
        } else {
            name.ends_with(suffix)
        }
    })
}

fn is_direct_child(dir: &Path, file: &Path) -> bool {
    file.parent().map(|p| p == dir).unwrap_or(false)
}

fn is_excluded(path: &Path, target: &Path, exclude: &[String]) -> bool {
    if exclude.is_empty() {
        return false;
    }

    let basename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

    let rel = path
        .strip_prefix(target)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    for pattern in exclude {
        if matches_glob(pattern, basename) || matches_glob(pattern, &rel) {
            return true;
        }
    }
    false
}

/// Minimal glob matching (supports `*` and `?` wildcards).
fn matches_glob(pattern: &str, text: &str) -> bool {
    if pattern == text {
        return true;
    }
    if !pattern.contains('*') && !pattern.contains('?') {
        return false;
    }

    let pi = pattern.chars().peekable();
    let ti = text.chars().peekable();

    fn match_inner(
        mut pi: std::iter::Peekable<std::str::Chars<'_>>,
        mut ti: std::iter::Peekable<std::str::Chars<'_>>,
    ) -> bool {
        loop {
            match (pi.peek(), ti.peek()) {
                (None, None) => return true,
                (Some('*'), _) => {
                    pi.next();
                    if pi.peek().is_none() {
                        return true;
                    }
                    while ti.peek().is_some() {
                        if match_inner(pi.clone(), ti.clone()) {
                            return true;
                        }
                        ti.next();
                    }
                    return false;
                }
                (Some('?'), Some(_)) => {
                    pi.next();
                    ti.next();
                }
                (Some(p), Some(t)) if *p == *t => {
                    pi.next();
                    ti.next();
                }
                _ => return false,
            }
        }
    }

    match_inner(pi, ti)
}

/// Convert an absolute path to a repo-relative POSIX path.
pub fn to_posix_rel(abs_path: &Path, repo_root: &Path) -> String {
    abs_path
        .strip_prefix(repo_root)
        .unwrap_or(abs_path)
        .to_string_lossy()
        .replace('\\', "/")
}
