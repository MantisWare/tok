//! Regression baseline for the pre-existing `tok mem` surface.
//!
//! These snapshots pin the output of every `tok mem` subcommand as it behaved
//! before the code-graph work started. Later phases swap the extractor and add
//! new commands; if any of these snapshots move, an existing contract broke.
//!
//! Values that legitimately vary between runs (temp paths, timestamps, episode
//! ids) are normalized. Symbol ids are deliberately *not* normalized — the
//! `sha256(repo_id:file_path:name:kind)[:16]` formula is a frozen contract
//! because `episodes.symbol_id` references it.

use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use assert_fs::TempDir;

use super::tok_cmd;

const REPO_ID: &str = "baseline";

/// A fixture repo copied outside the tok checkout, plus an isolated database.
struct Fixture {
    dir: TempDir,
    db: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().expect("create temp dir");
        let src = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("code_graph");
        copy_tree(&src, dir.path());
        let db = dir.path().join("memory.db");
        Self { dir, db }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    /// Run `tok mem <args>` against the isolated fixture and return stdout.
    fn mem(&self, args: &[&str]) -> String {
        let mut cmd = tok_cmd();
        cmd.arg("mem")
            .args(args)
            .current_dir(self.root())
            .env("TOK_MEMORY_DB_PATH", &self.db)
            .env("TOK_TELEMETRY_DISABLED", "1")
            .env("NO_COLOR", "1");
        let out = cmd.output().expect("run tok mem");
        self.normalize(&String::from_utf8_lossy(&out.stdout))
    }

    /// Strip anything that legitimately differs between runs.
    fn normalize(&self, raw: &str) -> String {
        let root = self.root().to_string_lossy().to_string();
        // macOS hands out /var/... paths that canonicalize to /private/var/...
        let canonical = std::fs::canonicalize(self.root())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| root.clone());

        let mut out = raw.replace(&canonical, "<FIXTURE>");
        out = out.replace(&root, "<FIXTURE>");
        out = replace_timestamps(&out);
        out = replace_episode_ids(&out);
        out.trim_end().to_string()
    }
}

/// Replace RFC-3339-ish timestamps with a stable placeholder.
fn replace_timestamps(input: &str) -> String {
    let bytes: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        if looks_like_timestamp(&bytes[i..]) {
            out.push_str("<TS>");
            // Consume through the end of the timestamp token.
            while i < bytes.len() && !bytes[i].is_whitespace() {
                i += 1;
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

/// A timestamp starts with `YYYY-MM-DD` followed by `T` or a space-less tail.
fn looks_like_timestamp(rest: &[char]) -> bool {
    if rest.len() < 10 {
        return false;
    }
    let digits = |c: char| c.is_ascii_digit();
    digits(rest[0])
        && digits(rest[1])
        && digits(rest[2])
        && digits(rest[3])
        && rest[4] == '-'
        && digits(rest[5])
        && digits(rest[6])
        && rest[7] == '-'
        && digits(rest[8])
        && digits(rest[9])
}

/// Episode ids are `ep_` + 14 hex characters.
fn replace_episode_ids(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let is_episode = chars[i] == 'e'
            && i + 3 < chars.len()
            && chars[i + 1] == 'p'
            && chars[i + 2] == '_'
            && chars[i + 3].is_ascii_hexdigit();
        if is_episode {
            out.push_str("<EPISODE>");
            i += 3;
            while i < chars.len() && chars[i].is_ascii_hexdigit() {
                i += 1;
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn copy_tree(src: &Path, dst: &Path) {
    for entry in std::fs::read_dir(src).expect("read fixture dir") {
        let entry = entry.expect("fixture entry");
        let target = dst.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            std::fs::create_dir_all(&target).expect("create fixture subdir");
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("copy fixture file");
        }
    }
}

/// Index once, then exercise every subcommand against the same database.
///
/// One test rather than eighteen: indexing is the expensive step and the
/// subcommands all read the state it produces, so sharing it keeps the suite
/// fast and guarantees they all observe an identical database.
#[test]
fn mem_surface_baseline() {
    let fx = Fixture::new();
    let mut report = String::new();

    let mut record = |label: &str, body: String| {
        report.push_str("=== ");
        report.push_str(label);
        report.push_str(" ===\n");
        report.push_str(&body);
        report.push_str("\n\n");
    };

    record(
        "index",
        fx.mem(&[
            "index",
            fx.root().to_str().expect("utf-8 fixture path"),
            "--repo-id",
            REPO_ID,
        ]),
    );
    record("repos", fx.mem(&["repos"]));
    // `status` and `forget` take a positional repo id; everything else uses --repo-id.
    record("status", fx.mem(&["status", REPO_ID]));
    record("search", fx.mem(&["search", "cache", "--repo-id", REPO_ID]));
    record(
        "search-kind-filter",
        fx.mem(&[
            "search",
            "cache",
            "--repo-id",
            REPO_ID,
            "--kind",
            "Function",
        ]),
    );
    record("find", fx.mem(&["find", "Cache", "--repo-id", REPO_ID]));
    record(
        "find-fuzzy",
        fx.mem(&["find", "cach", "--fuzzy", "--repo-id", REPO_ID]),
    );
    record(
        "context",
        fx.mem(&["context", "Cache", "--repo-id", REPO_ID]),
    );
    record(
        "relations",
        fx.mem(&["relations", "Cache", "--repo-id", REPO_ID]),
    );
    record(
        "relations-class-hierarchy",
        fx.mem(&[
            "relations",
            "Cache",
            "--query-type",
            "class_hierarchy",
            "--repo-id",
            REPO_ID,
        ]),
    );
    record("impact", fx.mem(&["impact", "Cache", "--repo-id", REPO_ID]));
    record("central", fx.mem(&["central", "--repo-id", REPO_ID]));
    record("bridges", fx.mem(&["bridges", "--repo-id", REPO_ID]));
    record(
        "communities",
        fx.mem(&["communities", "--repo-id", REPO_ID]),
    );
    record("dead-code", fx.mem(&["dead-code", "--repo-id", REPO_ID]));
    record("complexity", fx.mem(&["complexity", "--repo-id", REPO_ID]));
    record(
        "complexity-threshold-1",
        fx.mem(&["complexity", "--repo-id", REPO_ID, "--min-complexity", "1"]),
    );
    record(
        "detect",
        fx.mem(&["detect", "rust/lib.rs", "--repo-id", REPO_ID]),
    );
    record(
        "timeline",
        fx.mem(&["timeline", "Cache", "--repo-id", REPO_ID]),
    );
    record(
        "evolution",
        fx.mem(&[
            "evolution",
            "--repo-id",
            REPO_ID,
            "--from",
            "2000-01-01T00:00:00Z",
            "--to",
            "2100-01-01T00:00:00Z",
        ]),
    );
    record("changes", fx.mem(&["changes", "--repo-id", REPO_ID]));
    record("missing-symbol", fx.mem(&["context", "NoSuchSymbol"]));
    record("forget", fx.mem(&["forget", REPO_ID]));
    record("repos-after-forget", fx.mem(&["repos"]));

    insta::assert_snapshot!("mem_surface", report);
}

/// Exit codes are part of the contract: not-found paths return 1, not 0.
#[test]
fn mem_exit_codes_baseline() {
    let fx = Fixture::new();
    fx.mem(&[
        "index",
        fx.root().to_str().expect("utf-8 fixture path"),
        "--repo-id",
        REPO_ID,
    ]);

    let code = |args: &[&str]| -> i32 {
        let mut cmd = tok_cmd();
        cmd.arg("mem")
            .args(args)
            .current_dir(fx.root())
            .env("TOK_MEMORY_DB_PATH", &fx.db)
            .env("TOK_TELEMETRY_DISABLED", "1")
            .env("NO_COLOR", "1");
        cmd.output()
            .expect("run tok mem")
            .status
            .code()
            .unwrap_or(-1)
    };

    assert_eq!(code(&["context", "MissingSymbol"]), 1, "context not-found");
    assert_eq!(
        code(&["relations", "MissingSymbol"]),
        1,
        "relations not-found"
    );
    assert_eq!(code(&["impact", "MissingSymbol"]), 1, "impact not-found");
    assert_eq!(code(&["status", "nope"]), 1, "status not-found");
    assert_eq!(code(&["forget", "nope"]), 1, "forget not-found");
    assert_eq!(
        code(&["timeline", "MissingSymbol"]),
        1,
        "timeline not-found"
    );
    assert_eq!(
        code(&["changes", "--repo-id", "nope"]),
        1,
        "changes not-found"
    );
    assert_eq!(
        code(&[
            "evolution",
            "--repo-id",
            "nope",
            "--from",
            "2000-01-01T00:00:00Z",
            "--to",
            "2100-01-01T00:00:00Z",
        ]),
        1,
        "evolution not-found"
    );

    assert_eq!(code(&["repos"]), 0, "repos always succeeds");
    assert_eq!(code(&["search", "cache"]), 0, "search always succeeds");
    assert_eq!(code(&["find", "cache"]), 0, "find always succeeds");
}

/// `--incremental` currently only skips the pre-index clear. It still re-parses
/// everything and leaves rows behind for deleted files. Phase 2 fixes the stale
/// rows; this pins today's behaviour so the change is visible in review rather
/// than silent.
#[test]
fn mem_incremental_leaves_stale_rows_baseline() {
    let fx = Fixture::new();
    let root = fx.root().to_str().expect("utf-8 fixture path").to_string();

    fx.mem(&["index", &root, "--repo-id", REPO_ID]);
    let before = fx.mem(&["find", "slugify", "--repo-id", REPO_ID]);
    assert!(
        before.contains("slugify"),
        "fixture should define slugify, got: {before}"
    );

    std::fs::remove_file(fx.root().join("ts").join("util.ts")).expect("remove fixture file");
    std::fs::remove_file(fx.root().join("python").join("util.py")).expect("remove fixture file");
    fx.mem(&["index", &root, "--repo-id", REPO_ID, "--incremental"]);

    let after = fx.mem(&["find", "slugify", "--repo-id", REPO_ID]);
    assert!(
        after.contains("slugify"),
        "BASELINE: incremental indexing leaves symbols for deleted files. \
         If this assertion fails, Phase 2 fixed it — update the test and note \
         the behaviour change in CHANGELOG.md. Got: {after}"
    );
}

/// Guard against the fixture silently going missing or losing a language.
#[test]
fn fixture_covers_every_indexed_language() {
    let fx = Fixture::new();
    for rel in [
        "rust/lib.rs",
        "ts/cache.ts",
        "ts/util.ts",
        "python/cache.py",
        "python/util.py",
        "go/cache.go",
    ] {
        assert!(
            fx.root().join(rel).exists(),
            "fixture file {rel} is missing"
        );
    }
}

/// The fixture must live outside any git repo, otherwise `evolution`/`timeline`
/// would pick up the surrounding checkout's history and the snapshot would
/// change on every commit.
#[test]
fn fixture_is_not_inside_a_git_repo() {
    let fx = Fixture::new();
    let out = StdCommand::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(fx.root())
        .output();
    if let Ok(out) = out {
        if out.status.success() {
            let toplevel = String::from_utf8_lossy(&out.stdout).trim().to_string();
            panic!("fixture landed inside a git repo at {toplevel}; snapshots would be unstable");
        }
    }
}
