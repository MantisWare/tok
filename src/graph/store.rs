//! Layout of `.tok/graph/` and the atomic file primitives that write into it.
//!
//! Every path is derived from the *indexed repo root* rather than the process
//! working directory. Indexing another directory must not scatter cache files
//! into wherever the user happened to be standing.
//!
//! Writes go through [`write_atomic`]: a temp file in the same directory
//! followed by a rename. A half-written `graph.json` is worse than a missing
//! one, because a missing file rebuilds while a truncated one fails to parse
//! on every subsequent run.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Directory holding the graph and its caches, relative to a repo root.
pub const GRAPH_DIR: &str = ".tok/graph";

/// Resolved locations for one repository's graph data.
#[derive(Debug, Clone)]
pub struct GraphPaths {
    pub root: PathBuf,
}

impl GraphPaths {
    pub fn new(repo_root: impl AsRef<Path>) -> Self {
        Self {
            root: repo_root.as_ref().join(GRAPH_DIR),
        }
    }

    /// The serialized [`crate::graph::GraphV1`].
    pub fn graph(&self) -> PathBuf {
        self.root.join("graph.json")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.root.join(".cache")
    }

    /// Pre-tokenized retrieval sidecar. Derived from the graph, so it lives
    /// beside it rather than in `.cache/`, and is rebuilt whenever it disagrees
    /// with the graph's extractor stamp.
    pub fn ask_index(&self) -> PathBuf {
        self.root.join("ask-index.json")
    }

    /// Per-file drift probe data. Keyed by extractor stamp so an extractor
    /// change cannot be mistaken for unchanged files.
    pub fn fingerprints(&self, stamp: &str) -> PathBuf {
        self.cache_dir()
            .join(format!("fingerprint.{}.json", sanitize(stamp)))
    }

    /// Memoized per-file extraction output.
    pub fn extract_cache(&self, stamp: &str) -> PathBuf {
        self.cache_dir()
            .join(format!("extract.{}.json", sanitize(stamp)))
    }

    pub fn lock(&self) -> PathBuf {
        self.cache_dir().join(".sync.lock")
    }

    /// Create the directory tree, including the cache subdirectory.
    pub fn ensure(&self) -> Result<()> {
        fs::create_dir_all(self.cache_dir())
            .with_context(|| format!("Failed to create {}", self.cache_dir().display()))?;
        Ok(())
    }

    pub fn exists(&self) -> bool {
        self.graph().exists()
    }
}

/// Make a stamp safe for use inside a filename.
///
/// Stamps contain `/` (`tok-graph/1/ts`), which would otherwise be read as a
/// path separator and silently write outside the cache directory.
fn sanitize(stamp: &str) -> String {
    stamp
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Write bytes to `path` atomically, creating parent directories as needed.
pub fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    // The temp file must share a directory with the target: rename is only
    // atomic within a filesystem, and /tmp is often a different one.
    let temp = path.with_extension(format!("tmp-{}", std::process::id()));

    {
        let mut file = fs::File::create(&temp)
            .with_context(|| format!("Failed to create {}", temp.display()))?;
        file.write_all(contents)
            .with_context(|| format!("Failed to write {}", temp.display()))?;
        file.flush()?;
    }

    fs::rename(&temp, path).with_context(|| {
        format!(
            "Failed to move {} into place at {}",
            temp.display(),
            path.display()
        )
    })?;

    Ok(())
}

/// Read a JSON file, returning `None` when it is absent or unreadable.
///
/// A corrupt cache is treated as a cache miss rather than an error: these
/// files are all regenerable, and failing an index because a cache went bad
/// would be a worse outcome than rebuilding it.
pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Serialize as pretty JSON and write atomically.
///
/// Pretty rather than compact because `[graph] commit_graph = true` makes
/// these files reviewable, and a one-symbol change should not rewrite one
/// enormous line.
pub fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    let json = serde_json::to_vec_pretty(value).context("Failed to serialize graph data")?;
    write_atomic(path, &json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn paths_are_repo_relative() {
        let paths = GraphPaths::new("/repo");
        assert!(paths.graph().ends_with(".tok/graph/graph.json"));
        assert!(paths.cache_dir().ends_with(".tok/graph/.cache"));
    }

    #[test]
    fn stamps_with_slashes_stay_inside_the_cache_directory() {
        let paths = GraphPaths::new("/repo");
        let fp = paths.fingerprints("tok-graph/1/ts");

        assert_eq!(fp.parent(), Some(paths.cache_dir().as_path()));
        assert!(
            !fp.file_name()
                .and_then(|n| n.to_str())
                .expect("utf-8 filename")
                .contains('/'),
            "separator must not survive into the filename"
        );
    }

    #[test]
    fn different_stamps_get_different_cache_files() {
        let paths = GraphPaths::new("/repo");
        assert_ne!(paths.fingerprints("a"), paths.fingerprints("b"));
        assert_ne!(paths.extract_cache("a"), paths.fingerprints("a"));
    }

    #[test]
    fn atomic_write_creates_missing_directories() {
        let dir = TempDir::new().expect("tempdir");
        let target = dir.path().join("a/b/c.json");

        write_atomic(&target, b"hello").expect("write");
        assert_eq!(fs::read_to_string(&target).expect("read"), "hello");
    }

    #[test]
    fn atomic_write_replaces_existing_content_completely() {
        let dir = TempDir::new().expect("tempdir");
        let target = dir.path().join("f.json");

        write_atomic(&target, b"a longer original value").expect("write");
        write_atomic(&target, b"short").expect("overwrite");

        assert_eq!(fs::read_to_string(&target).expect("read"), "short");
    }

    #[test]
    fn atomic_write_leaves_no_temp_files_behind() {
        let dir = TempDir::new().expect("tempdir");
        write_atomic(&dir.path().join("f.json"), b"x").expect("write");

        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .expect("read dir")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|name| name.contains("tmp"))
            .collect();

        assert!(leftovers.is_empty(), "found {leftovers:?}");
    }

    #[test]
    fn json_round_trips() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("v.json");

        write_json(&path, &vec![1u32, 2, 3]).expect("write");
        let back: Vec<u32> = read_json(&path).expect("read");
        assert_eq!(back, vec![1, 2, 3]);
    }

    #[test]
    fn a_corrupt_cache_reads_as_a_miss() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("v.json");
        fs::write(&path, b"{not json").expect("write");

        assert!(read_json::<Vec<u32>>(&path).is_none());
    }

    #[test]
    fn a_missing_cache_reads_as_a_miss() {
        let dir = TempDir::new().expect("tempdir");
        assert!(read_json::<Vec<u32>>(&dir.path().join("nope.json")).is_none());
    }
}
