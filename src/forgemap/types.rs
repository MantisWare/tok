//! Core data types for the ForgeMap code-indexing and annotation engine.
//!
//! These mirror the TypeScript types from the ForgeMap protocol spec (FORGEMAP.md §3),
//! adapted for Rust with `String`-based keys and `BTreeMap` for deterministic ordering.

use std::collections::BTreeMap;
use std::path::PathBuf;

/// Repo-relative POSIX path (forward slashes, even on Windows).
pub type RelPath = String;

/// Public symbol exported from a file, stored as a human-readable signature string.
pub type ExportSig = String;

/// Internal map: importer file -> \[imported symbols\]. Empty vec means "imports the file
/// but no specific named symbol".
pub type DepMap = BTreeMap<RelPath, Vec<String>>;

/// Reverse map: imported file -> { importer file -> \[symbols\] }.
pub type UsedByMap = BTreeMap<RelPath, BTreeMap<RelPath, Vec<String>>>;

/// Information extracted from a single source file.
#[derive(Debug, Clone)]
pub struct FileInfo {
    /// Absolute path on disk.
    pub abs_path: PathBuf,
    /// Repo-relative POSIX path (the canonical key everywhere).
    pub rel: RelPath,
    /// Public exports as printable signatures.
    pub exports: Vec<ExportSig>,
    /// Map of dep file (rel path) -> imported symbols.
    pub deps: DepMap,
    /// Parsed ForgeMap fields (if a header was found), else `None`.
    pub header: Option<ParsedHeader>,
    /// `true` iff a ForgeMap header was detected.
    pub has_forgemap: bool,
    /// `false` if the file failed to parse.
    pub parseable: bool,
}

/// A parsed ForgeMap comment header from the top of a source file.
///
/// Stores raw strings for round-trip fidelity — only `exports` and `used_by`
/// are ever rewritten by `refresh`.
#[derive(Debug, Clone)]
pub struct ParsedHeader {
    /// First line minus the leading comment prefix — the purpose blurb.
    pub first_line: String,
    /// Raw value of `exports:` (joined into a single string for round-tripping).
    pub exports: String,
    /// Raw value of `used_by:` — may be multi-line, lines joined with `\n`.
    pub used_by: String,
    pub related: Option<String>,
    pub wiki: Option<String>,
    pub rules: String,
    pub agent: String,
    pub message: Option<String>,
    /// 0-based line index where the header block starts.
    pub start_line: usize,
    /// 0-based line index where it ends (the last comment line of the block).
    pub end_line: usize,
}

/// Detected package (directory subtree containing source files).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PackageInfo {
    /// Path-like key, e.g. `"src/services/"` or `""` for repo root.
    pub key: String,
    pub files: Vec<RelPath>,
    pub purpose: String,
    /// Bare basenames ranked by importance.
    pub key_files: Vec<String>,
    /// Other package keys this package depends on, sorted.
    pub depends_on: Vec<String>,
}

/// A single agent session entry for the `.forgemap` manifest.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AgentSession {
    pub agent: String,
    pub provider: String,
    /// YYYY-MM-DD
    pub date: String,
    /// e.g. `"s_20260429_001"`
    pub session_id: String,
    /// Task description (max 15 words).
    pub task: String,
    pub changed: Vec<RelPath>,
    pub visited: Vec<RelPath>,
    /// Free-form narrative.
    pub message: String,
}

/// The full `.forgemap` project manifest.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Manifest {
    pub project: String,
    pub description: String,
    pub mode: ManifestMode,
    pub packages: BTreeMap<String, PackageManifestEntry>,
    /// Preserved verbatim across runs.
    pub cross_cutting_patterns_block: String,
    /// Append-only, rolling window of last 3.
    pub agent_sessions: Vec<AgentSession>,
}

/// Package entry as serialized in the manifest (no `key` or full file list).
#[derive(Debug, Clone)]
pub struct PackageManifestEntry {
    pub purpose: String,
    pub key_files: Vec<String>,
    pub depends_on: Vec<String>,
}

/// Manifest operation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestMode {
    Human,
    Semi,
    Agent,
}

#[allow(dead_code)]
impl ManifestMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Semi => "semi",
            Self::Agent => "agent",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "human" => Self::Human,
            "agent" => Self::Agent,
            _ => Self::Semi,
        }
    }
}

/// Previously-read manifest fields for merge during writes.
#[derive(Debug, Clone, Default)]
pub struct ExistingManifest {
    pub project: String,
    pub description: String,
    pub mode: String,
    pub agent_sessions_block: String,
    pub cross_cutting_block: String,
}

/// Options controlling init / update / refresh / check pipeline runs.
#[derive(Debug, Clone)]
pub struct InitOptions {
    /// Absolute path — file or directory.
    pub target: PathBuf,
    /// Absolute path to the repository root.
    pub repo_root: PathBuf,
    /// File extensions to include (default: `SUPPORTED_EXTENSIONS`).
    pub extensions: Option<Vec<String>>,
    /// Glob patterns to exclude.
    pub exclude: Vec<String>,
    /// Preview changes without writing.
    pub dry_run: bool,
    /// Re-annotate already-annotated files (init only).
    pub force: bool,
    /// Verbose output.
    pub verbose: bool,
    /// Model ID for the `agent:` line. Default: `"forgemap-cli (no-llm)"`.
    pub model_id: String,
    /// Session ID. Default: auto-generated.
    pub session_id: String,
}

impl Default for InitOptions {
    fn default() -> Self {
        Self {
            target: PathBuf::from("."),
            repo_root: PathBuf::from("."),
            extensions: None,
            exclude: Vec::new(),
            dry_run: false,
            force: false,
            verbose: false,
            model_id: crate::forgemap::constants::DEFAULT_MODEL_ID.to_string(),
            session_id: String::new(),
        }
    }
}

/// Result of an init / update run.
#[derive(Debug, Clone, Default)]
pub struct InitResult {
    pub total_files: usize,
    pub annotated: usize,
    pub skipped: usize,
    pub errors: usize,
}

/// Result of a check run.
#[derive(Debug, Clone, Default)]
pub struct CheckResult {
    pub total_files: usize,
    pub annotated: usize,
    pub missing: Vec<RelPath>,
    pub unparseable: Vec<RelPath>,
    pub all_annotated: bool,
}

/// Result of a refresh on a single file.
#[derive(Debug, Clone)]
pub struct RefreshFileResult {
    pub source: String,
    pub changed: bool,
    pub changed_fields: Vec<String>,
}

/// Aggregate result of a refresh run.
#[derive(Debug, Clone, Default)]
pub struct RefreshResult {
    pub total_files: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub skipped_no_header: usize,
    pub errors: usize,
}

/// Options for the wiki bootstrap command.
#[derive(Debug, Clone)]
pub struct WikiBootstrapOptions {
    pub target: PathBuf,
    pub repo_root: PathBuf,
    pub out_dir: PathBuf,
    pub extensions: Option<Vec<String>>,
    pub exclude: Vec<String>,
    pub verbose: bool,
}

/// Options for the wiki sync command.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct WikiSyncOptions {
    pub repo_root: PathBuf,
    pub out_path: PathBuf,
    pub verbose: bool,
}

/// Options for the install command.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct InstallOptions {
    pub repo_root: PathBuf,
    /// Which tool prompts to install: `"claude"`, `"cursor"`, `"copilot"`.
    pub tools: Vec<String>,
    pub verbose: bool,
}

/// Options for the manifest command.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ManifestOptions {
    pub target: PathBuf,
    pub repo_root: PathBuf,
    pub extensions: Option<Vec<String>>,
    pub exclude: Vec<String>,
    pub dry_run: bool,
    pub verbose: bool,
    pub model_id: String,
    pub session_id: String,
}
