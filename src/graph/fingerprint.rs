//! Cheap detection of which files changed since the last index.
//!
//! Hashing every file on every run defeats the point of an incremental index —
//! reading the bytes *is* most of the cost. So the probe works in two tiers:
//!
//! 1. **Fast path.** Size and mtime both match the recorded values, so the file
//!    is assumed unchanged. This is the overwhelmingly common case and costs
//!    one `stat` per file.
//! 2. **Suspect path.** Either differs, so the contents are hashed. An edit
//!    that preserves size *and* mtime is the only thing this misses, which
//!    means a deliberately backdated write — and `TOK_GRAPH_REFRESH=hash`
//!    exists for anyone who needs to rule that out.
//!
//! mtime alone would be too eager: checkouts and `touch` rewrite it without
//! changing content, and re-extracting an untouched file is pure waste.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Recorded state of one file at its last extraction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileFingerprint {
    pub size: u64,
    /// Milliseconds since the Unix epoch. Milliseconds rather than nanoseconds
    /// because filesystem timestamp resolution varies and finer precision would
    /// produce spurious mismatches across platforms.
    pub mtime_ms: u64,
    /// Hex SHA-256 of the contents.
    pub hash: String,
}

/// Fingerprints for a whole repository, keyed by repo-relative path.
///
/// `BTreeMap` so the serialized cache is byte-stable across runs.
pub type Fingerprints = BTreeMap<String, FileFingerprint>;

/// How thoroughly to check for changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriftMode {
    /// Size and mtime first, hashing only what looks suspect.
    Fast,
    /// Hash every file. Slower, but immune to timestamp games.
    Hash,
}

impl DriftMode {
    /// `TOK_GRAPH_REFRESH=hash` opts into full hashing.
    pub fn from_env() -> Self {
        match std::env::var("TOK_GRAPH_REFRESH").as_deref() {
            Ok("hash") => DriftMode::Hash,
            _ => DriftMode::Fast,
        }
    }
}

/// What the probe concluded about one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    /// Fingerprint matched; reuse the cached extraction.
    Unchanged,
    /// Contents differ, or the file is new.
    Changed,
}

/// Hex SHA-256 of arbitrary bytes.
pub fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Read a file's size and mtime without reading its contents.
pub fn stat(path: &Path) -> Option<(u64, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis() as u64;
    Some((meta.len(), mtime))
}

/// Build a fingerprint from known contents.
pub fn fingerprint_of(contents: &[u8], size: u64, mtime_ms: u64) -> FileFingerprint {
    FileFingerprint {
        size,
        mtime_ms,
        hash: hash_bytes(contents),
    }
}

/// Decide whether a file changed, reading its contents only if necessary.
///
/// `load_contents` is called at most once, and only when the cheap check is
/// inconclusive, so callers can pass a closure that does real I/O.
pub fn probe<F>(
    previous: Option<&FileFingerprint>,
    size: u64,
    mtime_ms: u64,
    mode: DriftMode,
    load_contents: F,
) -> (FileStatus, Option<String>)
where
    F: FnOnce() -> Option<Vec<u8>>,
{
    let Some(prev) = previous else {
        // Unknown file: hash it so the next run has a fast path to take.
        let hash = load_contents().map(|b| hash_bytes(&b));
        return (FileStatus::Changed, hash);
    };

    if mode == DriftMode::Fast && prev.size == size && prev.mtime_ms == mtime_ms {
        return (FileStatus::Unchanged, Some(prev.hash.clone()));
    }

    // Size differs, mtime moved, or the caller demanded certainty. A moved
    // mtime is not proof of an edit — compare content hashes before deciding.
    let Some(bytes) = load_contents() else {
        return (FileStatus::Changed, None);
    };

    let hash = hash_bytes(&bytes);
    let status = if hash == prev.hash {
        FileStatus::Unchanged
    } else {
        FileStatus::Changed
    };

    (status, Some(hash))
}

/// Paths present in `previous` but absent from `current`.
///
/// Deleted files are the defect in today's `--incremental`: their rows survive
/// in SQLite forever, so `dead-code` and `search` keep reporting symbols from
/// files that no longer exist.
pub fn removed_paths(previous: &Fingerprints, current: &[String]) -> Vec<String> {
    let present: std::collections::HashSet<&str> = current.iter().map(String::as_str).collect();
    previous
        .keys()
        .filter(|p| !present.contains(p.as_str()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(size: u64, mtime_ms: u64, contents: &str) -> FileFingerprint {
        FileFingerprint {
            size,
            mtime_ms,
            hash: hash_bytes(contents.as_bytes()),
        }
    }

    #[test]
    fn an_unknown_file_is_changed() {
        let (status, hash) = probe(None, 10, 100, DriftMode::Fast, || Some(b"hi".to_vec()));
        assert_eq!(status, FileStatus::Changed);
        assert_eq!(hash, Some(hash_bytes(b"hi")));
    }

    #[test]
    fn matching_size_and_mtime_skips_reading_the_file() {
        let prev = fp(10, 100, "body");
        let mut read = false;

        let (status, _) = probe(Some(&prev), 10, 100, DriftMode::Fast, || {
            read = true;
            Some(b"body".to_vec())
        });

        assert_eq!(status, FileStatus::Unchanged);
        assert!(!read, "the fast path must not read the file");
    }

    #[test]
    fn a_changed_size_is_investigated_and_reported() {
        let prev = fp(4, 100, "body");
        let (status, hash) = probe(Some(&prev), 7, 100, DriftMode::Fast, || {
            Some(b"changed".to_vec())
        });

        assert_eq!(status, FileStatus::Changed);
        assert_eq!(hash, Some(hash_bytes(b"changed")));
    }

    /// A checkout or `touch` moves mtime without changing bytes. Re-extracting
    /// then would waste most of the work an incremental index is meant to save.
    #[test]
    fn a_touched_but_unmodified_file_is_unchanged() {
        let prev = fp(4, 100, "body");
        let (status, _) = probe(Some(&prev), 4, 999_999, DriftMode::Fast, || {
            Some(b"body".to_vec())
        });

        assert_eq!(status, FileStatus::Unchanged);
    }

    #[test]
    fn hash_mode_reads_even_when_stat_matches() {
        let prev = fp(4, 100, "body");
        let mut read = false;

        let (status, _) = probe(Some(&prev), 4, 100, DriftMode::Hash, || {
            read = true;
            Some(b"body".to_vec())
        });

        assert!(read, "hash mode must not trust stat");
        assert_eq!(status, FileStatus::Unchanged);
    }

    /// The one case the fast path misses, and the reason `TOK_GRAPH_REFRESH`
    /// exists.
    #[test]
    fn hash_mode_catches_an_edit_that_preserved_size_and_mtime() {
        let prev = fp(4, 100, "body");

        let (fast, _) = probe(Some(&prev), 4, 100, DriftMode::Fast, || {
            Some(b"BODY".to_vec())
        });
        assert_eq!(fast, FileStatus::Unchanged, "fast path cannot see this");

        let (thorough, _) = probe(Some(&prev), 4, 100, DriftMode::Hash, || {
            Some(b"BODY".to_vec())
        });
        assert_eq!(thorough, FileStatus::Changed);
    }

    #[test]
    fn an_unreadable_file_counts_as_changed() {
        let prev = fp(4, 100, "body");
        let (status, hash) = probe(Some(&prev), 9, 100, DriftMode::Fast, || None);

        assert_eq!(status, FileStatus::Changed);
        assert_eq!(hash, None);
    }

    #[test]
    fn deleted_files_are_detected() {
        let mut previous = Fingerprints::new();
        previous.insert("a.rs".into(), fp(1, 1, "a"));
        previous.insert("b.rs".into(), fp(1, 1, "b"));

        let removed = removed_paths(&previous, &["a.rs".to_string()]);
        assert_eq!(removed, vec!["b.rs".to_string()]);
    }

    #[test]
    fn nothing_is_removed_when_every_file_survives() {
        let mut previous = Fingerprints::new();
        previous.insert("a.rs".into(), fp(1, 1, "a"));

        assert!(removed_paths(&previous, &["a.rs".to_string()]).is_empty());
    }

    #[test]
    fn drift_mode_defaults_to_fast() {
        // Reads the ambient environment; the default must hold when unset.
        if std::env::var("TOK_GRAPH_REFRESH").is_err() {
            assert_eq!(DriftMode::from_env(), DriftMode::Fast);
        }
    }
}
