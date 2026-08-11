//! Auto-refresh and the lock that keeps concurrent rebuilds from colliding.
//!
//! An agent editing files and then asking a question expects the answer to
//! reflect the edit, so query commands refresh the graph before reading it.
//! But several agents — or an editor hook and a terminal — can ask at once, and
//! two simultaneous rebuilds would fight over the same cache files.
//!
//! The protocol is a lock file with a bounded wait: a second caller waits up to
//! [`LOCK_WAIT_MS`], polling every [`LOCK_POLL_MS`], and if the holder finishes
//! in time it simply reads the graph that holder produced. Waiting forever
//! would hang an agent behind a crashed process, so the wait is capped and a
//! stale lock is broken rather than obeyed.
//!
//! **Refresh never runs on the filter hot path.** `tok rewrite`, `tok hook *`,
//! and every command filter are excluded by construction: they never call into
//! this module. A graph rebuild must never sit between an agent and the command
//! it asked to run.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::Result;

use crate::graph::store::GraphPaths;

/// How long a second caller waits for the holder to finish.
pub const LOCK_WAIT_MS: u64 = 2000;

/// Polling interval while waiting.
pub const LOCK_POLL_MS: u64 = 50;

/// A lock older than this is assumed to belong to a dead process.
///
/// Generous relative to [`LOCK_WAIT_MS`] because a large repository's first
/// index legitimately takes a while, and breaking a live lock is far worse
/// than waiting out a dead one.
pub const LOCK_STALE_MS: u64 = 60_000;

/// Why a refresh was skipped, for callers that report it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// `TOK_GRAPH_NO_REFRESH` is set.
    Disabled,
    /// Another process held the lock and did not finish in time.
    Busy,
}

/// Whether auto-refresh is permitted in this process.
///
/// Set `TOK_GRAPH_NO_REFRESH=1` to pin the graph — useful in CI, and for
/// measuring query latency without a rebuild in the sample.
pub fn is_enabled() -> bool {
    !matches!(
        std::env::var("TOK_GRAPH_NO_REFRESH").as_deref(),
        Ok("1") | Ok("true")
    )
}

/// An acquired lock, released when dropped.
///
/// Drop-based release matters because a query can fail anywhere — a missing
/// grammar, an unreadable file — and an early return must not leave the lock
/// behind for the stale timeout to clean up.
#[derive(Debug)]
pub struct RefreshLock {
    path: PathBuf,
}

impl RefreshLock {
    /// Try to take the lock, waiting for a holder to finish.
    ///
    /// Returns `Ok(None)` when someone else holds it and is still working, which
    /// the caller should treat as "read the graph as it stands" rather than as
    /// an error.
    pub fn acquire(paths: &GraphPaths) -> Result<Option<Self>> {
        Self::acquire_with_timeout(paths, Duration::from_millis(LOCK_WAIT_MS))
    }

    pub fn acquire_with_timeout(paths: &GraphPaths, wait: Duration) -> Result<Option<Self>> {
        paths.ensure()?;
        let path = paths.lock();
        let deadline = std::time::Instant::now() + wait;

        loop {
            if try_create(&path)? {
                return Ok(Some(Self { path }));
            }

            if is_stale(&path) {
                // The holder died mid-build. Removing the lock can race another
                // waiter doing the same thing, which is harmless: whoever wins
                // the next create owns it, and the loser keeps waiting.
                let _ = std::fs::remove_file(&path);
                continue;
            }

            if std::time::Instant::now() >= deadline {
                return Ok(None);
            }

            std::thread::sleep(Duration::from_millis(LOCK_POLL_MS));
        }
    }

    /// Take the lock only if it is free, without waiting.
    pub fn try_acquire(paths: &GraphPaths) -> Result<Option<Self>> {
        Self::acquire_with_timeout(paths, Duration::ZERO)
    }
}

impl Drop for RefreshLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Create the lock file, failing if it already exists.
///
/// `create_new` is the atomic primitive here: check-then-create would let two
/// processes both observe an absent lock and both proceed.
fn try_create(path: &Path) -> Result<bool> {
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            use std::io::Write;
            // The pid is diagnostic only — nothing reads it to make decisions,
            // since a pid can be recycled.
            let _ = writeln!(file, "{}", std::process::id());
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// Whether a lock is old enough to have been abandoned.
fn is_stale(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        // Vanished between checks; treat as free rather than stale.
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };

    SystemTime::now()
        .duration_since(modified)
        .map(|age| age.as_millis() as u64 > LOCK_STALE_MS)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn paths(dir: &TempDir) -> GraphPaths {
        GraphPaths::new(dir.path())
    }

    #[test]
    fn acquires_when_free() {
        let dir = TempDir::new().expect("tempdir");
        let lock = RefreshLock::try_acquire(&paths(&dir)).expect("acquire");
        assert!(lock.is_some());
    }

    #[test]
    fn a_second_caller_is_refused_while_held() {
        let dir = TempDir::new().expect("tempdir");
        let p = paths(&dir);

        let _held = RefreshLock::try_acquire(&p).expect("first").expect("held");
        let second = RefreshLock::try_acquire(&p).expect("second");

        assert!(second.is_none(), "must not run two rebuilds at once");
    }

    #[test]
    fn dropping_the_lock_releases_it() {
        let dir = TempDir::new().expect("tempdir");
        let p = paths(&dir);

        {
            let _held = RefreshLock::try_acquire(&p).expect("first").expect("held");
        }

        assert!(
            RefreshLock::try_acquire(&p).expect("second").is_some(),
            "lock should be free after drop"
        );
    }

    /// A query that fails partway must not strand the lock for a minute.
    #[test]
    fn a_panicking_holder_still_releases() {
        let dir = TempDir::new().expect("tempdir");
        let p = paths(&dir);
        let lock_path = p.lock();

        let result = std::panic::catch_unwind({
            let p = p.clone();
            move || {
                let _held = RefreshLock::try_acquire(&p)
                    .expect("acquire")
                    .expect("held");
                panic!("simulated failure mid-refresh");
            }
        });

        assert!(result.is_err(), "the panic should have propagated");
        assert!(!lock_path.exists(), "drop must have run during unwind");
    }

    #[test]
    fn waiting_gives_up_after_the_timeout() {
        let dir = TempDir::new().expect("tempdir");
        let p = paths(&dir);

        let _held = RefreshLock::try_acquire(&p).expect("first").expect("held");

        let start = std::time::Instant::now();
        let second =
            RefreshLock::acquire_with_timeout(&p, Duration::from_millis(120)).expect("second");

        assert!(second.is_none());
        assert!(
            start.elapsed() >= Duration::from_millis(100),
            "should have actually waited"
        );
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "should not have waited the full default"
        );
    }

    #[test]
    fn a_waiter_acquires_once_the_holder_finishes() {
        let dir = TempDir::new().expect("tempdir");
        let p = paths(&dir);

        let held = RefreshLock::try_acquire(&p).expect("first").expect("held");

        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(80));
            drop(held);
        });

        let second = RefreshLock::acquire_with_timeout(&p, Duration::from_millis(1500))
            .expect("second")
            .is_some();

        releaser.join().expect("thread");
        assert!(second, "should have taken the lock after release");
    }

    #[test]
    fn a_stale_lock_is_broken() {
        let dir = TempDir::new().expect("tempdir");
        let p = paths(&dir);
        p.ensure().expect("mkdir");

        std::fs::write(p.lock(), "99999").expect("write lock");

        // Backdate well past the stale threshold, simulating a killed process.
        let old = SystemTime::now() - Duration::from_millis(LOCK_STALE_MS * 2);
        std::fs::File::options()
            .write(true)
            .open(p.lock())
            .expect("open lock")
            .set_times(std::fs::FileTimes::new().set_modified(old))
            .expect("backdate");

        let lock = RefreshLock::try_acquire(&p).expect("acquire");
        assert!(lock.is_some(), "a dead holder must not block forever");
    }

    #[test]
    fn a_fresh_lock_is_not_stale() {
        let dir = TempDir::new().expect("tempdir");
        let p = paths(&dir);
        p.ensure().expect("mkdir");
        std::fs::write(p.lock(), "1").expect("write lock");

        assert!(!is_stale(&p.lock()));
    }

    #[test]
    fn refresh_is_enabled_by_default() {
        if std::env::var("TOK_GRAPH_NO_REFRESH").is_err() {
            assert!(is_enabled());
        }
    }
}
