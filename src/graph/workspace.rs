//! Multi-repo federation.
//!
//! A workspace is a parent directory holding several sibling repositories and
//! no source of its own — the `~/GIT/acme/` that contains `acme/api`,
//! `acme/web`, and `acme/shared`. Agents work across all three, and asking a
//! question in the parent should search all three rather than fail because the
//! parent contains no code.
//!
//! Each child keeps its own independent graph under its own `.tok/graph/`. That
//! is the whole design: a child indexed as part of a workspace is byte-identical
//! to the same child indexed alone, so federation is a query-time concern only
//! and nothing about it can corrupt a child's data. The parent stores a single
//! `workspace.json` listing its children, and no graph at all.
//!
//! The safeguard that matters is [`is_workspace_root`]. A repository with git
//! submodules also has repo-shaped children, and treating one as a workspace
//! would silently stop indexing the parent's own code. So a parent qualifies
//! only when it has no git directory of its own.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::graph::store::GraphPaths;

/// Fewer children than this is not a workspace, it is a directory that happens
/// to contain a checkout.
const MIN_CHILDREN: usize = 2;

/// Directories that hold checkouts but are never themselves interesting.
const SKIP_DIRS: &[&str] = &["node_modules", "vendor", "target", "dist", ".git"];

/// The parent's entire on-disk state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub version: u32,
    /// Immediate subdirectory names, sorted.
    pub children: Vec<String>,
}

impl Workspace {
    pub fn new(children: Vec<String>) -> Self {
        let mut children = children;
        children.sort();
        children.dedup();
        Self {
            version: 1,
            children,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }
}

/// Where the parent's workspace index lives.
pub fn manifest_path(root: &Path) -> PathBuf {
    GraphPaths::new(root).root.join("workspace.json")
}

/// Read a parent's workspace index, if it has one.
pub fn read(root: &Path) -> Option<Workspace> {
    crate::graph::store::read_json::<Workspace>(&manifest_path(root))
        .filter(|workspace| !workspace.is_empty())
}

/// Write the parent's workspace index.
pub fn write(root: &Path, workspace: &Workspace) -> Result<()> {
    let paths = GraphPaths::new(root);
    paths.ensure().context("Cannot create .tok/graph")?;
    crate::graph::store::write_json(&manifest_path(root), workspace)
}

/// Whether indexing this directory should split into per-child builds.
///
/// The `.git` check is what separates a workspace from a repository with
/// submodules. Without it, indexing a submodule-using repo would quietly stop
/// covering that repo's own source.
pub fn is_workspace_root(root: &Path) -> bool {
    if read(root).is_some() {
        return true;
    }

    !root.join(".git").exists() && children(root).len() >= MIN_CHILDREN
}

/// Immediate subdirectories that are themselves git repositories, sorted.
pub fn children(root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };

    let mut found: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) {
                return None;
            }
            entry.path().join(".git").exists().then_some(name)
        })
        .collect();

    found.sort();
    found
}

/// The children to query, preferring the recorded list over a fresh scan.
///
/// Reading the manifest first means a child that was removed from the workspace
/// deliberately stays out of results until the parent is re-indexed, rather than
/// reappearing because it is still on disk.
pub fn members(root: &Path) -> Vec<String> {
    match read(root) {
        Some(workspace) => workspace
            .children
            .into_iter()
            .filter(|child| root.join(child).join(".git").exists())
            .collect(),
        None => children(root),
    }
}

/// Record the current children as the workspace index.
pub fn refresh(root: &Path) -> Result<Workspace> {
    let workspace = Workspace::new(children(root));
    write(root, &workspace)?;
    Ok(workspace)
}

/// Label a child's path for display at the parent, so a pointer copied out of
/// a federated answer still resolves from where the query was run.
pub fn federated_path(child: &str, path: &str) -> String {
    format!("{child}/{path}")
}

/// Label a hit that came from `child`'s `scope`.
///
/// A child's root scope is shown as just the child name: `api` rather than
/// `api/`, which would read as a directory inside `api`.
pub fn federated_scope(child: &str, scope: &str) -> String {
    if scope.is_empty() {
        child.to_string()
    } else {
        format!("{child}/{scope}")
    }
}

/// Split a `--in` value into the child repository and the scope within it.
///
/// `--in api/billing` means "the billing scope of the api repo". A value naming
/// no known child is left whole, so it can still match a path prefix inside a
/// single-repo query.
pub fn split_in<'a>(value: &'a str, members: &[String]) -> (Option<&'a str>, Option<&'a str>) {
    let normalized = value.trim_matches('/');
    let (head, rest) = match normalized.split_once('/') {
        Some((head, rest)) => (head, Some(rest)),
        None => (normalized, None),
    };

    if members.iter().any(|child| child == head) {
        return (Some(head), rest.filter(|r| !r.is_empty()));
    }

    (None, Some(normalized))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn repo(root: &Path, name: &str) {
        std::fs::create_dir_all(root.join(name).join(".git")).expect("mkdir");
    }

    fn names(list: &[String]) -> Vec<&str> {
        list.iter().map(String::as_str).collect()
    }

    #[test]
    fn sibling_repositories_are_discovered_as_children() {
        let dir = temp();
        repo(dir.path(), "api");
        repo(dir.path(), "web");

        assert_eq!(names(&children(dir.path())), vec!["api", "web"]);
    }

    #[test]
    fn a_directory_without_a_git_dir_is_not_a_child() {
        let dir = temp();
        repo(dir.path(), "api");
        std::fs::create_dir_all(dir.path().join("notes")).expect("mkdir");

        assert_eq!(names(&children(dir.path())), vec!["api"]);
    }

    #[test]
    fn vendor_directories_are_never_children() {
        let dir = temp();
        repo(dir.path(), "api");
        repo(dir.path(), "node_modules");
        repo(dir.path(), ".cache");

        assert_eq!(names(&children(dir.path())), vec!["api"]);
    }

    #[test]
    fn a_parent_holding_several_repositories_is_a_workspace() {
        let dir = temp();
        repo(dir.path(), "api");
        repo(dir.path(), "web");

        assert!(is_workspace_root(dir.path()));
    }

    /// The check that keeps submodules working: a repo with repo-shaped
    /// children is still a repo, and must keep indexing its own source.
    #[test]
    fn a_repository_with_submodules_is_not_a_workspace() {
        let dir = temp();
        std::fs::create_dir_all(dir.path().join(".git")).expect("mkdir");
        repo(dir.path(), "vendored-a");
        repo(dir.path(), "vendored-b");

        assert!(!is_workspace_root(dir.path()));
    }

    #[test]
    fn one_child_is_not_enough_to_federate() {
        let dir = temp();
        repo(dir.path(), "api");

        assert!(!is_workspace_root(dir.path()));
    }

    #[test]
    fn an_empty_directory_is_not_a_workspace() {
        assert!(!is_workspace_root(temp().path()));
    }

    /// Once recorded, the parent stays a workspace even while children are
    /// temporarily missing, so a query does not silently change meaning.
    #[test]
    fn a_recorded_workspace_stays_one_without_rescanning() {
        let dir = temp();
        std::fs::create_dir_all(dir.path().join(".git")).expect("mkdir");
        write(dir.path(), &Workspace::new(vec!["api".to_string()])).expect("write");

        assert!(is_workspace_root(dir.path()));
    }

    #[test]
    fn the_recorded_index_round_trips() {
        let dir = temp();
        let workspace = Workspace::new(vec!["web".to_string(), "api".to_string()]);

        write(dir.path(), &workspace).expect("write");

        assert_eq!(read(dir.path()), Some(workspace));
    }

    #[test]
    fn children_are_recorded_in_a_stable_order() {
        let workspace = Workspace::new(vec!["web".to_string(), "api".to_string()]);

        assert_eq!(names(&workspace.children), vec!["api", "web"]);
    }

    #[test]
    fn an_absent_index_reads_as_nothing() {
        assert_eq!(read(temp().path()), None);
    }

    #[test]
    fn an_empty_index_reads_as_nothing() {
        let dir = temp();
        write(dir.path(), &Workspace::new(Vec::new())).expect("write");

        assert_eq!(read(dir.path()), None);
    }

    #[test]
    fn refreshing_records_what_is_on_disk_now() {
        let dir = temp();
        repo(dir.path(), "api");
        repo(dir.path(), "web");

        let workspace = refresh(dir.path()).expect("refresh");

        assert_eq!(names(&workspace.children), vec!["api", "web"]);
        assert_eq!(read(dir.path()), Some(workspace));
    }

    /// A child removed from the workspace should stay out of results until the
    /// parent is re-indexed, even though it is still on disk.
    #[test]
    fn members_come_from_the_recorded_index_not_a_fresh_scan() {
        let dir = temp();
        repo(dir.path(), "api");
        repo(dir.path(), "web");
        write(dir.path(), &Workspace::new(vec!["api".to_string()])).expect("write");

        assert_eq!(names(&members(dir.path())), vec!["api"]);
    }

    #[test]
    fn a_recorded_child_that_is_gone_is_not_queried() {
        let dir = temp();
        repo(dir.path(), "api");
        write(
            dir.path(),
            &Workspace::new(vec!["api".to_string(), "deleted".to_string()]),
        )
        .expect("write");

        assert_eq!(names(&members(dir.path())), vec!["api"]);
    }

    #[test]
    fn members_fall_back_to_scanning_when_nothing_is_recorded() {
        let dir = temp();
        repo(dir.path(), "api");
        repo(dir.path(), "web");

        assert_eq!(names(&members(dir.path())), vec!["api", "web"]);
    }

    #[test]
    fn a_federated_pointer_resolves_from_the_parent() {
        assert_eq!(federated_path("api", "src/main.rs"), "api/src/main.rs");
    }

    #[test]
    fn a_child_root_scope_is_labelled_with_just_the_child() {
        assert_eq!(federated_scope("api", ""), "api");
        assert_eq!(federated_scope("api", "billing"), "api/billing");
    }

    #[test]
    fn narrowing_splits_into_child_and_inner_scope() {
        let members = vec!["api".to_string(), "web".to_string()];

        assert_eq!(split_in("api", &members), (Some("api"), None));
        assert_eq!(
            split_in("api/billing", &members),
            (Some("api"), Some("billing"))
        );
        assert_eq!(split_in("api/", &members), (Some("api"), None));
    }

    /// A prefix naming no child is still a valid path filter within one repo.
    #[test]
    fn narrowing_on_an_unknown_name_stays_a_path_prefix() {
        let members = vec!["api".to_string()];

        assert_eq!(split_in("src/core", &members), (None, Some("src/core")));
    }
}
