//! Import specifier to file path resolution.
//!
//! Knowing that `./util` means `src/util.ts` and not one of the four other
//! files declaring `normalize` is the difference between a resolved call edge
//! and a dropped ambiguous one. Without this, any repo that reuses a common
//! helper name across modules loses most of its cross-file edges.
//!
//! Resolution is purely lexical — no filesystem access — so it stays cheap and
//! deterministic. Candidates are returned in preference order and the caller
//! keeps the first that actually exists in the graph.

/// Extensions tried for a bare specifier, in the order a bundler would.
const MODULE_EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx", "mjs", "cjs", "py", "go", "rs"];

/// Index files tried when a specifier names a directory.
const INDEX_STEMS: &[&str] = &["index", "__init__", "mod"];

/// Candidate repo-relative paths a specifier might refer to.
///
/// Returns an empty vector for package imports (`react`, `net/http`), which
/// point outside the repo and have no local file.
pub fn candidates(importing_file: &str, specifier: &str) -> Vec<String> {
    if specifier.is_empty() {
        return Vec::new();
    }

    let base = if is_relative(specifier) {
        let dir = parent_dir(importing_file);
        let Some(joined) = join_normalized(dir, specifier) else {
            return Vec::new();
        };
        joined
    } else if importing_file.ends_with(".py") && is_dotted_module(specifier) {
        // Python's `from a.b import c` addresses a path from some root; try it
        // next to the importer first, then from the repo root.
        let dotted = specifier.replace('.', "/");
        let mut out = Vec::new();
        if let Some(local) = join_normalized(parent_dir(importing_file), &dotted) {
            out.extend(expand(&local));
        }
        out.extend(expand(&dotted));
        return dedup(out);
    } else {
        return Vec::new();
    };

    dedup(expand(&base))
}

/// Whether a specifier addresses a path rather than a package.
fn is_relative(specifier: &str) -> bool {
    specifier.starts_with("./")
        || specifier.starts_with("../")
        || specifier == "."
        || specifier == ".."
}

/// A bare specifier that could be a Python module path rather than a package.
///
/// Only consulted for Python importers: `react` and `util` are
/// indistinguishable as strings, so the importing language decides.
fn is_dotted_module(specifier: &str) -> bool {
    !specifier.contains('/') && !specifier.starts_with('@')
}

/// Everything before the last slash, or empty for a root-level file.
fn parent_dir(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..i],
        None => "",
    }
}

/// Join a directory and a relative specifier, collapsing `.` and `..`.
///
/// Returns `None` when the specifier escapes above the repo root, which means
/// it refers to something outside the indexed tree.
fn join_normalized(dir: &str, specifier: &str) -> Option<String> {
    let mut parts: Vec<&str> = if dir.is_empty() {
        Vec::new()
    } else {
        dir.split('/').collect()
    };

    for segment in specifier.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }

    Some(parts.join("/"))
}

/// Expand a base path into the concrete files it could name.
fn expand(base: &str) -> Vec<String> {
    let mut out = Vec::new();

    // An explicit extension is used as-is.
    if has_known_extension(base) {
        out.push(base.to_string());
        return out;
    }

    for ext in MODULE_EXTENSIONS {
        out.push(format!("{base}.{ext}"));
    }
    for stem in INDEX_STEMS {
        for ext in MODULE_EXTENSIONS {
            out.push(format!("{base}/{stem}.{ext}"));
        }
    }

    out
}

fn has_known_extension(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, ext)| MODULE_EXTENSIONS.contains(&ext))
}

fn dedup(mut items: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    items.retain(|p| seen.insert(p.clone()));
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sibling_relative_import() {
        let out = candidates("src/cache.ts", "./util");
        assert!(out.contains(&"src/util.ts".to_string()));
        assert!(out.contains(&"src/util.js".to_string()));
    }

    #[test]
    fn parent_relative_import() {
        let out = candidates("src/deep/cache.ts", "../util");
        assert!(out.contains(&"src/util.ts".to_string()));
    }

    #[test]
    fn root_level_importer() {
        let out = candidates("cache.ts", "./util");
        assert!(out.contains(&"util.ts".to_string()));
    }

    #[test]
    fn directory_imports_try_index_files() {
        let out = candidates("src/cache.ts", "./models");
        assert!(out.contains(&"src/models/index.ts".to_string()));
        assert!(out.contains(&"src/models/__init__.py".to_string()));
    }

    #[test]
    fn explicit_extensions_are_kept() {
        let out = candidates("src/cache.ts", "./util.js");
        assert_eq!(out, vec!["src/util.js".to_string()]);
    }

    #[test]
    fn package_imports_have_no_local_candidates() {
        assert!(candidates("src/cache.ts", "react").is_empty());
        assert!(candidates("main.go", "net/http").is_empty());
        assert!(candidates("src/a.ts", "@scope/pkg").is_empty());
    }

    /// `react` and `util` are the same shape; only the importing language can
    /// tell a package apart from a sibling module.
    #[test]
    fn bare_specifiers_are_module_paths_only_for_python() {
        assert!(candidates("app/cache.py", "util").contains(&"app/util.py".to_string()));
        assert!(candidates("app/cache.ts", "util").is_empty());
    }

    #[test]
    fn python_dotted_modules_resolve_locally_and_from_root() {
        let out = candidates("app/cache.py", "util");
        assert!(out.contains(&"app/util.py".to_string()));
        assert!(out.contains(&"util.py".to_string()));
    }

    #[test]
    fn python_nested_packages_resolve() {
        let out = candidates("app/cache.py", "models.entry");
        assert!(out.contains(&"app/models/entry.py".to_string()));
        assert!(out.contains(&"models/entry.py".to_string()));
    }

    #[test]
    fn escaping_above_the_root_yields_nothing() {
        assert!(candidates("a.ts", "../../outside").is_empty());
    }

    #[test]
    fn candidates_are_deduplicated() {
        let out = candidates("app/cache.py", "cache");
        let mut sorted = out.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), out.len());
    }

    #[test]
    fn empty_specifier_is_ignored() {
        assert!(candidates("a.ts", "").is_empty());
    }
}
