//! Monorepo scope discovery.
//!
//! A scope is a sub-project inside one repository: `frontend/`, `packages/api/`,
//! a Cargo workspace member. Retrieval treats each as its own corpus, because a
//! monorepo indexed flat ranks badly — the term "user" is rare and therefore
//! high-signal inside `billing/`, yet common and meaningless across the whole
//! tree, and a single global IDF splits the difference in a way that helps
//! neither.
//!
//! Discovery is layout-driven rather than configured. It reads the marker files
//! a project already has (`package.json`, `Cargo.toml`, `go.mod`, …) plus any
//! declared workspace globs, so a correctly structured repo needs no TOK-specific
//! setup.
//!
//! Three guards keep this from over-splitting, which is the failure mode that
//! makes scoping worse than no scoping:
//!
//! - **Depth.** Nothing more than [`MAX_SCOPE_DEPTH`] segments deep becomes a
//!   scope unless a workspace glob explicitly names it.
//! - **Nesting.** Where scopes nest, the shallower one wins, so a package with
//!   its own `package.json` inside another package does not fragment it.
//! - **Substance.** A scope with fewer than [`MIN_SCOPE_NODES`] symbols folds
//!   back into the root, applied after the build since node counts are not known
//!   before then.
//!
//! A repository with no sub-projects yields exactly one root scope with an empty
//! prefix, and every scope-aware path then degenerates to the single-corpus
//! behaviour it had before this module existed.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::graph::types::{NodeKind, NodeV1};

/// Files that mark a directory as a project in its own right, in the order they
/// are recorded so that `markers` is deterministic.
const MARKERS: &[&str] = &[
    "package.json",
    "go.mod",
    "pyproject.toml",
    "setup.py",
    "Cargo.toml",
];

/// How deep a directory may sit and still become a scope on marker evidence
/// alone.
///
/// `packages/api/` is a scope; `packages/api/src/lib/` is not, even though it
/// may contain a stray `package.json`. A workspace glob overrides this, since
/// naming a directory in `workspaces` is an explicit statement that it is a
/// project.
const MAX_SCOPE_DEPTH: usize = 2;

/// Symbols a scope needs before it stands on its own.
///
/// Below this it is a stub — a config package, a barrel file — and giving it its
/// own corpus only creates a competitor in fusion that can never lose on
/// relevance because it has almost nothing to be irrelevant about.
const MIN_SCOPE_NODES: usize = 5;

/// One sub-project within a repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeV1 {
    /// Repo-relative directory prefix, forward-slashed, no trailing slash.
    /// Empty for the root scope.
    pub prefix: String,
    /// What the scope is called in output. Equal to `prefix` today; kept
    /// separate so a future rename cannot change path matching.
    pub label: String,
    /// Marker files found here, for explaining why this became a scope.
    #[serde(default)]
    pub markers: Vec<String>,
}

impl ScopeV1 {
    pub fn root() -> Self {
        Self {
            prefix: String::new(),
            label: String::new(),
            markers: Vec::new(),
        }
    }

    pub fn is_root(&self) -> bool {
        self.prefix.is_empty()
    }
}

/// The single-scope answer, used whenever discovery finds no sub-projects.
pub fn root_only() -> Vec<ScopeV1> {
    vec![ScopeV1::root()]
}

/// Discover scopes from the repo layout.
///
/// `files` are repo-relative source paths, normally the same set the build
/// walked, so discovery can only see directories that extraction also sees. A
/// directory full of ignored or generated files never becomes a scope.
pub fn discover(root: &Path, files: &[String]) -> Vec<ScopeV1> {
    let dirs = directories(files);
    let globs = workspace_globs(root);
    let matched: BTreeSet<String> = globs
        .iter()
        .flat_map(|glob| resolve_glob(&dirs, glob))
        .collect();

    let mut candidates: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for dir in &dirs {
        if dir.is_empty() {
            continue;
        }

        let markers = markers_in(root, dir);
        let in_workspace = matched.contains(dir);

        if markers.is_empty() && !in_workspace {
            continue;
        }
        // A glob naming this directory is explicit intent, and outranks the
        // depth heuristic that exists only to guess at unstated intent.
        if !in_workspace && depth(dir) > MAX_SCOPE_DEPTH {
            continue;
        }

        candidates.insert(dir.clone(), markers);
    }

    let kept = collapse_nested(candidates, &matched);

    if kept.is_empty() {
        return root_only();
    }

    let mut scopes: Vec<ScopeV1> = kept
        .into_iter()
        .map(|(prefix, markers)| ScopeV1 {
            label: prefix.clone(),
            prefix,
            markers,
        })
        .collect();

    // Root is always present as the owner of anything outside a sub-project:
    // top-level scripts, shared config, the odd file nobody moved.
    scopes.push(ScopeV1::root());
    sort_scopes(&mut scopes);
    scopes
}

/// Fold scopes too small to be worth ranking separately back into the root.
///
/// Runs after the build because it needs node counts, which do not exist until
/// extraction has finished.
pub fn apply_min_substance(scopes: Vec<ScopeV1>, nodes: &[NodeV1]) -> Vec<ScopeV1> {
    if scopes.len() <= 1 {
        return scopes;
    }

    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for node in nodes {
        // File nodes exist for every file regardless of content, so counting
        // them would let an empty directory clear the bar.
        if node.kind == NodeKind::File {
            continue;
        }
        counts
            .entry(scope_of(&node.file, &scopes).prefix.as_str())
            .and_modify(|c| *c += 1)
            .or_insert(1);
    }

    let mut kept: Vec<ScopeV1> = scopes
        .iter()
        .filter(|scope| {
            scope.is_root()
                || counts.get(scope.prefix.as_str()).copied().unwrap_or(0) >= MIN_SCOPE_NODES
        })
        .cloned()
        .collect();

    // Everything folded away, so there is no monorepo here after all.
    if kept.len() <= 1 {
        return root_only();
    }

    sort_scopes(&mut kept);
    kept
}

/// The scope owning a path: the longest matching prefix, root as the fallback.
///
/// `scopes` must be sorted by [`sort_scopes`], which puts longer prefixes first
/// so the first match is the most specific one.
pub fn scope_of<'a>(path: &str, scopes: &'a [ScopeV1]) -> &'a ScopeV1 {
    static ROOT: std::sync::OnceLock<ScopeV1> = std::sync::OnceLock::new();

    for scope in scopes {
        if scope.is_root() {
            continue;
        }
        if path_under_prefix(path, &scope.prefix) {
            return scope;
        }
    }

    scopes
        .iter()
        .find(|scope| scope.is_root())
        .unwrap_or_else(|| ROOT.get_or_init(ScopeV1::root))
}

/// Whether a path sits under a prefix, respecting segment boundaries.
///
/// `frontend` must not match `frontend-utils/main.ts`, which a plain
/// `starts_with` would.
pub fn path_under_prefix(path: &str, prefix: &str) -> bool {
    prefix.is_empty() || path == prefix || path.starts_with(&format!("{prefix}/"))
}

/// How a scope is named in output. The root has no prefix to show, so it gets a
/// word instead of an empty string.
pub fn scope_label(prefix: &str) -> String {
    if prefix.is_empty() {
        "(root)".to_string()
    } else {
        format!("{prefix}/")
    }
}

/// Normalize a user-supplied `--in` value into a comparable prefix.
pub fn normalize_prefix(input: &str) -> String {
    input
        .replace('\\', "/")
        .trim_start_matches("./")
        .trim_matches('/')
        .to_string()
}

/// Longest prefix first, then lexicographic, so [`scope_of`] can stop at its
/// first match and so output ordering is stable.
fn sort_scopes(scopes: &mut [ScopeV1]) {
    scopes.sort_by(|a, b| {
        b.prefix
            .len()
            .cmp(&a.prefix.len())
            .then_with(|| a.prefix.cmp(&b.prefix))
    });
}

/// Every directory containing at least one indexed file, plus their ancestors.
fn directories(files: &[String]) -> Vec<String> {
    let mut dirs: BTreeSet<String> = BTreeSet::new();

    for file in files {
        let mut current = file.as_str();
        while let Some(index) = current.rfind('/') {
            current = &current[..index];
            dirs.insert(current.to_string());
        }
    }

    dirs.into_iter().collect()
}

fn depth(dir: &str) -> usize {
    dir.split('/').filter(|s| !s.is_empty()).count()
}

fn markers_in(root: &Path, dir: &str) -> Vec<String> {
    MARKERS
        .iter()
        .filter(|marker| root.join(dir).join(marker).is_file())
        .map(|marker| (*marker).to_string())
        .collect()
}

/// Drop scopes nested inside other scopes, keeping the shallower one.
///
/// A workspace glob wins over a parent that only had marker evidence: naming
/// `packages/*` states that each package is a project, even though the
/// repository root also has a `package.json`.
fn collapse_nested(
    candidates: BTreeMap<String, Vec<String>>,
    matched: &BTreeSet<String>,
) -> Vec<(String, Vec<String>)> {
    let prefixes: Vec<String> = candidates.keys().cloned().collect();

    candidates
        .iter()
        .filter(|(dir, _)| {
            !prefixes.iter().any(|other| {
                // A glob match survives a non-glob ancestor, and loses to a
                // glob ancestor, so nesting inside a workspace still collapses.
                let ancestor_wins = !matched.contains(*dir) || matched.contains(other);

                other != *dir && path_under_prefix(dir, other) && ancestor_wins
            })
        })
        .map(|(dir, markers)| (dir.clone(), markers.clone()))
        .collect()
}

/// Workspace member globs declared by the repo root, across ecosystems.
fn workspace_globs(root: &Path) -> Vec<String> {
    let mut globs = Vec::new();

    globs.extend(pnpm_globs(root));
    globs.extend(package_json_globs(root));
    globs.extend(cargo_globs(root));
    globs.extend(go_work_globs(root));

    globs.sort();
    globs.dedup();
    globs
}

/// `pnpm-workspace.yaml`'s `packages:` list.
///
/// Read with a small hand-written parser rather than a YAML dependency: the
/// shape is one flat list of strings, and the file is otherwise irrelevant to
/// TOK.
fn pnpm_globs(root: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(root.join("pnpm-workspace.yaml")) else {
        return Vec::new();
    };

    let mut globs = Vec::new();
    let mut in_packages = false;

    for line in text.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("packages:") {
            in_packages = true;
            continue;
        }
        if !in_packages {
            continue;
        }

        if let Some(entry) = trimmed.strip_prefix("- ") {
            globs.push(entry.trim().trim_matches(['"', '\'']).to_string());
            continue;
        }
        // A non-indented, non-list line ends the block.
        if !trimmed.is_empty() && !line.starts_with([' ', '\t', '-']) {
            break;
        }
    }

    globs
}

/// `package.json`'s `workspaces`, in both the array and `{ packages: [] }`
/// forms npm and yarn accept.
fn package_json_globs(root: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(root.join("package.json")) else {
        return Vec::new();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };

    let workspaces = match json.get("workspaces") {
        Some(serde_json::Value::Array(list)) => list.clone(),
        Some(object) => match object.get("packages") {
            Some(serde_json::Value::Array(list)) => list.clone(),
            _ => return Vec::new(),
        },
        None => return Vec::new(),
    };

    workspaces
        .iter()
        .filter_map(|entry| entry.as_str().map(str::to_string))
        .collect()
}

/// `Cargo.toml`'s `[workspace] members`. Not something graft read, but TOK
/// indexes Rust, and a Cargo workspace is the same shape of statement.
fn cargo_globs(root: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(root.join("Cargo.toml")) else {
        return Vec::new();
    };
    let Ok(manifest) = toml::from_str::<toml::Value>(&text) else {
        return Vec::new();
    };

    manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("members"))
        .and_then(toml::Value::as_array)
        .map(|members| {
            members
                .iter()
                .filter_map(|member| member.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// `go.work`'s `use` directives, in both the single-line and block forms.
fn go_work_globs(root: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(root.join("go.work")) else {
        return Vec::new();
    };

    let mut globs = Vec::new();
    let mut in_block = false;

    for line in text.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("use (") {
            in_block = true;
            continue;
        }
        if in_block {
            if trimmed == ")" {
                in_block = false;
                continue;
            }
            push_go_path(&mut globs, trimmed);
            continue;
        }
        if let Some(entry) = trimmed.strip_prefix("use ") {
            push_go_path(&mut globs, entry);
        }
    }

    globs
}

fn push_go_path(globs: &mut Vec<String>, entry: &str) {
    let cleaned = entry.trim().trim_matches('"').trim_start_matches("./");
    if !cleaned.is_empty() && cleaned != "." {
        globs.push(cleaned.to_string());
    }
}

/// Expand one workspace glob against directories that actually exist.
///
/// Supports the forms workspace configs use in practice — `dir/*`, `dir/**`,
/// bare `*` and `**`, and literal paths. Anything more exotic resolves to
/// nothing, which costs a scope rather than producing a wrong one.
fn resolve_glob(dirs: &[String], pattern: &str) -> Vec<String> {
    let normalized = normalize_prefix(pattern);

    if normalized == "*" {
        return dirs.iter().filter(|d| depth(d) == 1).cloned().collect();
    }
    if normalized == "**" {
        return dirs.to_vec();
    }

    if let Some(base) = normalized.strip_suffix("/**") {
        return dirs
            .iter()
            .filter(|d| d.as_str() != base && path_under_prefix(d, base))
            .cloned()
            .collect();
    }
    if let Some(base) = normalized.strip_suffix("/*") {
        return dirs
            .iter()
            .filter(|d| path_under_prefix(d, base) && depth(d) == depth(base) + 1)
            .cloned()
            .collect();
    }

    // Any other wildcard is a form we do not claim to support.
    if normalized.contains('*') {
        return Vec::new();
    }

    dirs.iter()
        .filter(|d| d.as_str() == normalized)
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::Span;

    fn files(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|p| (*p).to_string()).collect()
    }

    fn write(root: &Path, relative: &str, contents: &str) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(path, contents).expect("write");
    }

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn node(file: &str) -> NodeV1 {
        NodeV1::new(
            format!("{file}#s"),
            NodeKind::Function,
            "sym".to_string(),
            file.to_string(),
            Span::new(1, 2),
        )
    }

    fn prefixes(scopes: &[ScopeV1]) -> Vec<&str> {
        scopes.iter().map(|s| s.prefix.as_str()).collect()
    }

    /// The common case, and the one that must stay exactly as fast and as
    /// simple as it was before scopes existed.
    #[test]
    fn a_plain_repository_has_one_root_scope() {
        let dir = temp();
        write(dir.path(), "src/main.rs", "");

        let scopes = discover(dir.path(), &files(&["src/main.rs"]));

        assert_eq!(scopes, root_only());
    }

    #[test]
    fn a_directory_with_a_marker_becomes_a_scope() {
        let dir = temp();
        write(dir.path(), "frontend/package.json", "{}");
        write(dir.path(), "backend/go.mod", "module x");

        let scopes = discover(
            dir.path(),
            &files(&["frontend/app.ts", "backend/main.go", "README.md"]),
        );

        assert_eq!(prefixes(&scopes), vec!["frontend", "backend", ""]);
    }

    #[test]
    fn the_marker_that_created_a_scope_is_recorded() {
        let dir = temp();
        write(dir.path(), "api/Cargo.toml", "");
        write(dir.path(), "web/package.json", "{}");

        let scopes = discover(dir.path(), &files(&["api/lib.rs", "web/app.ts"]));
        let api = scopes.iter().find(|s| s.prefix == "api").expect("api");

        assert_eq!(api.markers, vec!["Cargo.toml"]);
    }

    #[test]
    fn a_directory_without_a_marker_is_not_a_scope() {
        let dir = temp();
        write(dir.path(), "src/util.ts", "");

        let scopes = discover(dir.path(), &files(&["src/util.ts", "src/nested/deep.ts"]));

        assert_eq!(scopes, root_only());
    }

    /// Without this guard, one stray `package.json` in a source subdirectory
    /// fragments the repo.
    #[test]
    fn markers_below_the_depth_limit_are_ignored() {
        let dir = temp();
        write(dir.path(), "src/lib/inner/package.json", "{}");

        let scopes = discover(dir.path(), &files(&["src/lib/inner/a.ts"]));

        assert_eq!(scopes, root_only());
    }

    #[test]
    fn a_workspace_glob_overrides_the_depth_limit() {
        let dir = temp();
        write(
            dir.path(),
            "package.json",
            r#"{"workspaces":["libs/deep/*"]}"#,
        );
        write(dir.path(), "libs/deep/one/package.json", "{}");

        let scopes = discover(dir.path(), &files(&["libs/deep/one/index.ts"]));

        assert!(prefixes(&scopes).contains(&"libs/deep/one"));
    }

    #[test]
    fn pnpm_workspace_packages_become_scopes() {
        let dir = temp();
        write(
            dir.path(),
            "pnpm-workspace.yaml",
            "packages:\n  - 'packages/*'\n",
        );
        write(dir.path(), "packages/api/package.json", "{}");
        write(dir.path(), "packages/web/package.json", "{}");

        let scopes = discover(
            dir.path(),
            &files(&["packages/api/index.ts", "packages/web/index.ts"]),
        );

        assert_eq!(prefixes(&scopes), vec!["packages/api", "packages/web", ""]);
    }

    #[test]
    fn package_json_workspaces_accept_the_object_form() {
        let dir = temp();
        write(
            dir.path(),
            "package.json",
            r#"{"workspaces":{"packages":["apps/*"]}}"#,
        );
        write(dir.path(), "apps/site/package.json", "{}");

        let scopes = discover(dir.path(), &files(&["apps/site/index.ts"]));

        assert!(prefixes(&scopes).contains(&"apps/site"));
    }

    #[test]
    fn cargo_workspace_members_become_scopes() {
        let dir = temp();
        write(
            dir.path(),
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/core\", \"crates/cli\"]\n",
        );
        write(dir.path(), "crates/core/Cargo.toml", "");
        write(dir.path(), "crates/cli/Cargo.toml", "");

        let scopes = discover(
            dir.path(),
            &files(&["crates/core/lib.rs", "crates/cli/main.rs"]),
        );

        assert_eq!(prefixes(&scopes), vec!["crates/core", "crates/cli", ""]);
    }

    #[test]
    fn go_work_use_directives_become_scopes() {
        let dir = temp();
        write(
            dir.path(),
            "go.work",
            "go 1.22\n\nuse (\n\t./svc\n\t./gw\n)\n",
        );
        write(dir.path(), "svc/go.mod", "module svc");
        write(dir.path(), "gw/go.mod", "module gw");

        let scopes = discover(dir.path(), &files(&["svc/main.go", "gw/main.go"]));

        assert_eq!(prefixes(&scopes), vec!["svc", "gw", ""]);
    }

    #[test]
    fn a_single_line_go_work_use_is_read_too() {
        let dir = temp();
        write(dir.path(), "go.work", "go 1.22\nuse ./svc\n");
        write(dir.path(), "svc/go.mod", "module svc");

        let scopes = discover(dir.path(), &files(&["svc/main.go"]));

        assert!(prefixes(&scopes).contains(&"svc"));
    }

    #[test]
    fn a_nested_marker_does_not_split_its_parent_scope() {
        let dir = temp();
        write(dir.path(), "app/package.json", "{}");
        write(dir.path(), "app/sub/package.json", "{}");

        let scopes = discover(dir.path(), &files(&["app/a.ts", "app/sub/b.ts"]));

        assert_eq!(prefixes(&scopes), vec!["app", ""]);
    }

    /// The root having a `package.json` is normal in a monorepo and must not
    /// swallow the packages it declares.
    #[test]
    fn a_root_marker_does_not_swallow_workspace_packages() {
        let dir = temp();
        write(
            dir.path(),
            "package.json",
            r#"{"workspaces":["packages/*"]}"#,
        );
        write(dir.path(), "packages/api/package.json", "{}");

        let scopes = discover(dir.path(), &files(&["packages/api/index.ts", "index.ts"]));

        assert!(prefixes(&scopes).contains(&"packages/api"));
    }

    #[test]
    fn an_unsupported_glob_form_yields_no_scopes() {
        let dirs = files(&["packages/api", "packages/web"]);

        assert!(resolve_glob(&dirs, "packages/{api,web}").is_empty());
        assert!(resolve_glob(&dirs, "pack*ges/api").is_empty());
    }

    #[test]
    fn a_double_star_glob_matches_at_any_depth() {
        let dirs = files(&["libs", "libs/a", "libs/a/b", "other"]);

        assert_eq!(resolve_glob(&dirs, "libs/**"), vec!["libs/a", "libs/a/b"]);
    }

    #[test]
    fn a_single_star_glob_matches_only_one_level() {
        let dirs = files(&["libs", "libs/a", "libs/a/b"]);

        assert_eq!(resolve_glob(&dirs, "libs/*"), vec!["libs/a"]);
    }

    #[test]
    fn a_scope_with_too_few_symbols_folds_into_the_root() {
        let scopes = vec![
            ScopeV1 {
                prefix: "big".to_string(),
                label: "big".to_string(),
                markers: Vec::new(),
            },
            ScopeV1 {
                prefix: "stub".to_string(),
                label: "stub".to_string(),
                markers: Vec::new(),
            },
            ScopeV1::root(),
        ];
        let mut nodes: Vec<NodeV1> = (0..MIN_SCOPE_NODES)
            .map(|i| node(&format!("big/f{i}.ts")))
            .collect();
        nodes.push(node("stub/only.ts"));

        let kept = apply_min_substance(scopes, &nodes);

        assert_eq!(prefixes(&kept), vec!["big", ""]);
    }

    /// File nodes exist for every file whatever it contains, so counting them
    /// would let an empty package clear the bar.
    #[test]
    fn file_nodes_do_not_count_towards_substance() {
        let scopes = vec![
            ScopeV1 {
                prefix: "empty".to_string(),
                label: "empty".to_string(),
                markers: Vec::new(),
            },
            ScopeV1::root(),
        ];
        let nodes: Vec<NodeV1> = (0..20)
            .map(|i| {
                NodeV1::new(
                    format!("empty/f{i}.ts"),
                    NodeKind::File,
                    format!("f{i}.ts"),
                    format!("empty/f{i}.ts"),
                    Span::new(1, 1),
                )
            })
            .collect();

        assert_eq!(apply_min_substance(scopes, &nodes), root_only());
    }

    #[test]
    fn folding_every_scope_away_leaves_a_plain_root() {
        let scopes = vec![
            ScopeV1 {
                prefix: "a".to_string(),
                label: "a".to_string(),
                markers: Vec::new(),
            },
            ScopeV1::root(),
        ];

        assert_eq!(apply_min_substance(scopes, &[]), root_only());
    }

    #[test]
    fn the_longest_matching_prefix_owns_a_path() {
        let mut scopes = vec![
            ScopeV1 {
                prefix: "packages".to_string(),
                label: "packages".to_string(),
                markers: Vec::new(),
            },
            ScopeV1 {
                prefix: "packages/api".to_string(),
                label: "packages/api".to_string(),
                markers: Vec::new(),
            },
            ScopeV1::root(),
        ];
        sort_scopes(&mut scopes);

        assert_eq!(
            scope_of("packages/api/index.ts", &scopes).prefix,
            "packages/api"
        );
        assert_eq!(scope_of("packages/other.ts", &scopes).prefix, "packages");
    }

    #[test]
    fn a_path_outside_every_scope_belongs_to_the_root() {
        let scopes = discover(temp().path(), &files(&["README.md"]));

        assert!(scope_of("README.md", &scopes).is_root());
    }

    #[test]
    fn prefix_matching_respects_segment_boundaries() {
        assert!(path_under_prefix("frontend/app.ts", "frontend"));
        assert!(path_under_prefix("frontend", "frontend"));
        assert!(!path_under_prefix("frontend-utils/app.ts", "frontend"));
        assert!(path_under_prefix("anything", ""));
    }

    #[test]
    fn the_root_scope_reads_as_a_word_rather_than_an_empty_label() {
        assert_eq!(scope_label(""), "(root)");
        assert_eq!(scope_label("api"), "api/");
    }

    #[test]
    fn user_supplied_prefixes_are_normalized_before_comparison() {
        assert_eq!(normalize_prefix("./packages/api/"), "packages/api");
        assert_eq!(normalize_prefix("packages\\api"), "packages/api");
        assert_eq!(normalize_prefix("/api/"), "api");
        assert_eq!(normalize_prefix(""), "");
    }

    #[test]
    fn discovery_is_stable_across_runs() {
        let dir = temp();
        write(dir.path(), "a/package.json", "{}");
        write(dir.path(), "b/package.json", "{}");
        let paths = files(&["a/x.ts", "b/y.ts"]);

        let first = discover(dir.path(), &paths);
        for _ in 0..5 {
            assert_eq!(discover(dir.path(), &paths), first);
        }
    }

    #[test]
    fn a_malformed_marker_file_does_not_break_discovery() {
        let dir = temp();
        write(dir.path(), "package.json", "{ not json");
        write(dir.path(), "Cargo.toml", "[[[");
        write(dir.path(), "app/package.json", "{}");

        let scopes = discover(dir.path(), &files(&["app/a.ts"]));

        assert!(prefixes(&scopes).contains(&"app"));
    }
}
