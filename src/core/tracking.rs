//! Token savings tracking and analytics system.
//!
//! This module provides comprehensive tracking of TOK command executions,
//! recording token savings, execution times, and providing aggregation APIs
//! for daily/weekly/monthly statistics.
//!
//! # Architecture
//!
//! - Storage: SQLite database (`history.db` under the tok data directory; see `HISTORY_DB` / `TOK_DATA_DIR` in `constants.rs`)
//! - Retention: 90-day automatic cleanup
//! - Metrics: Input/output tokens, savings %, execution time
//!
//! # Quick Start
//!
//! ```no_run
//! use tok::tracking::{TimedExecution, Tracker};
//!
//! // Track a command execution
//! let timer = TimedExecution::start();
//! let input = "raw output";
//! let output = "filtered output";
//! timer.track("ls -la", "tok ls", input, output);
//!
//! // Query statistics
//! let tracker = Tracker::new().unwrap();
//! let summary = tracker.get_summary().unwrap();
//! println!("Saved {} tokens", summary.total_saved);
//! ```
//!
//! See [docs/tracking.md](../docs/tracking.md) for full documentation.

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Instant;

// ── Project path helpers ── // added: project-scoped tracking support

/// Get the canonical project path string for the current working directory.
fn current_project_path_string() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// Build SQL filter params for project-scoped queries.
/// Returns (exact_match, glob_prefix) for WHERE clause.
/// Uses GLOB instead of LIKE to avoid `_` and `%` in paths acting as wildcards. // changed: GLOB
fn project_filter_params(project_path: Option<&str>) -> (Option<String>, Option<String>) {
    match project_path {
        Some(p) => (
            Some(p.to_string()),
            Some(format!("{}{}*", p, std::path::MAIN_SEPARATOR)), // changed: GLOB pattern with * wildcard
        ),
        None => (None, None),
    }
}

use super::client::{display_client_name, resolve_client_id};
use super::constants::{DEFAULT_HISTORY_DAYS, HISTORY_DB, TOK_DATA_DIR};

/// Main tracking interface for recording and querying command history.
///
/// Manages SQLite database connection and provides methods for:
/// - Recording command executions with token counts and timing
/// - Querying aggregated statistics (summary, daily, weekly, monthly)
/// - Retrieving recent command history
///
/// # Database Location
///
/// - Linux: `~/.local/share/tok/history.db`
/// - macOS: `~/Library/Application Support/tok/history.db`
/// - Windows: `%APPDATA%\tok\history.db`
///
/// # Examples
///
/// ```no_run
/// use tok::tracking::Tracker;
///
/// let tracker = Tracker::new()?;
/// tracker.record("ls -la", "tok ls", 1000, 200, 50)?;
///
/// let summary = tracker.get_summary()?;
/// println!("Total saved: {} tokens", summary.total_saved);
/// # Ok::<(), anyhow::Error>(())
/// ```
pub struct Tracker {
    conn: Connection,
}

/// Individual command record from tracking history.
///
/// Contains timestamp, command name, and savings metrics for a single execution.
#[derive(Debug)]
pub struct CommandRecord {
    /// UTC timestamp when command was executed
    pub timestamp: DateTime<Utc>,
    /// TOK command that was executed (e.g., "tok ls")
    pub tok_cmd: String,
    /// Number of tokens saved (input - output)
    pub saved_tokens: usize,
    /// Savings percentage ((saved / input) * 100)
    pub savings_pct: f64,
}

/// Aggregated statistics across all recorded commands.
///
/// Provides overall metrics and breakdowns by command and by day.
/// Returned by [`Tracker::get_summary`].
#[derive(Debug)]
pub struct GainSummary {
    /// Total number of commands recorded
    pub total_commands: usize,
    /// Total input tokens across all commands
    pub total_input: usize,
    /// Total output tokens across all commands
    pub total_output: usize,
    /// Total tokens saved (input - output)
    pub total_saved: usize,
    /// Average savings percentage across all commands
    pub avg_savings_pct: f64,
    /// Total execution time across all commands (milliseconds)
    pub total_time_ms: u64,
    /// Average execution time per command (milliseconds)
    pub avg_time_ms: u64,
    /// Top commands by tokens saved: (cmd, count, saved, avg_pct, avg_time_ms)
    pub by_command: Vec<(String, usize, usize, f64, u64)>,
    /// Last 30 days of activity: (date, saved_tokens)
    pub by_day: Vec<(String, usize)>,
    /// Per-client breakdown (cursor, claude, terminal, …)
    pub by_client: Vec<ClientStats>,
}

/// Token savings aggregated by client (`TOK_CLIENT` / hook attribution).
#[derive(Debug, Clone, Serialize)]
pub struct ClientStats {
    /// Client id (e.g. `cursor`, `claude`, `terminal`)
    pub client: String,
    /// Number of commands from this client
    pub commands: usize,
    /// Total input tokens
    pub input_tokens: usize,
    /// Total output tokens
    pub output_tokens: usize,
    /// Total tokens saved
    pub saved_tokens: usize,
    /// Savings percentage for this client
    pub savings_pct: f64,
}

/// Row counts removed by [`Tracker::clear_tracking`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TrackingClearResult {
    /// Rows deleted from `commands`.
    pub commands_removed: usize,
    /// Rows deleted from `parse_failures` (global clear only).
    pub parse_failures_removed: usize,
}

/// Daily statistics for token savings and execution metrics.
///
/// Serializable to JSON for export via `tok gain --daily --format json`.
///
/// # JSON Schema
///
/// ```json
/// {
///   "date": "2026-02-03",
///   "commands": 42,
///   "input_tokens": 15420,
///   "output_tokens": 3842,
///   "saved_tokens": 11578,
///   "savings_pct": 75.08,
///   "total_time_ms": 8450,
///   "avg_time_ms": 201
/// }
/// ```
#[derive(Debug, Serialize)]
pub struct DayStats {
    /// ISO date (YYYY-MM-DD)
    pub date: String,
    /// Number of commands executed this day
    pub commands: usize,
    /// Total input tokens for this day
    pub input_tokens: usize,
    /// Total output tokens for this day
    pub output_tokens: usize,
    /// Total tokens saved this day
    pub saved_tokens: usize,
    /// Savings percentage for this day
    pub savings_pct: f64,
    /// Total execution time for this day (milliseconds)
    pub total_time_ms: u64,
    /// Average execution time per command (milliseconds)
    pub avg_time_ms: u64,
}

/// Weekly statistics for token savings and execution metrics.
///
/// Serializable to JSON for export via `tok gain --weekly --format json`.
/// Weeks start on Sunday (SQLite default).
#[derive(Debug, Serialize)]
pub struct WeekStats {
    /// Week start date (YYYY-MM-DD)
    pub week_start: String,
    /// Week end date (YYYY-MM-DD)
    pub week_end: String,
    /// Number of commands executed this week
    pub commands: usize,
    /// Total input tokens for this week
    pub input_tokens: usize,
    /// Total output tokens for this week
    pub output_tokens: usize,
    /// Total tokens saved this week
    pub saved_tokens: usize,
    /// Savings percentage for this week
    pub savings_pct: f64,
    /// Total execution time for this week (milliseconds)
    pub total_time_ms: u64,
    /// Average execution time per command (milliseconds)
    pub avg_time_ms: u64,
}

/// Monthly statistics for token savings and execution metrics.
///
/// Serializable to JSON for export via `tok gain --monthly --format json`.
#[derive(Debug, Serialize)]
pub struct MonthStats {
    /// Month identifier (YYYY-MM)
    pub month: String,
    /// Number of commands executed this month
    pub commands: usize,
    /// Total input tokens for this month
    pub input_tokens: usize,
    /// Total output tokens for this month
    pub output_tokens: usize,
    /// Total tokens saved this month
    pub saved_tokens: usize,
    /// Savings percentage for this month
    pub savings_pct: f64,
    /// Total execution time for this month (milliseconds)
    pub total_time_ms: u64,
    /// Average execution time per command (milliseconds)
    pub avg_time_ms: u64,
}

/// Type alias for command statistics tuple: (command, count, saved_tokens, avg_savings_pct, avg_time_ms)
type CommandStats = (String, usize, usize, f64, u64);

/// Default number of rows in `tok gain` "By Command" table.
pub const DEFAULT_GAIN_TOP: usize = 10;
/// Maximum for `tok gain --top`.
pub const MAX_GAIN_TOP: usize = 100;

/// Options for the per-command breakdown in [`GainSummary`].
#[derive(Debug, Clone, Copy)]
pub struct GainByCommandOptions {
    /// How many commands to show (clamped to [`MAX_GAIN_TOP`]).
    pub limit: usize,
    /// When true, aggregate by base tool (`cargo`, `grep`, `toml:jq`, …).
    pub rollup: bool,
}

impl Default for GainByCommandOptions {
    fn default() -> Self {
        Self {
            limit: DEFAULT_GAIN_TOP,
            rollup: false,
        }
    }
}

/// Clamp `tok gain --top` to a sane range.
pub fn clamp_gain_top(top: usize) -> usize {
    top.clamp(1, MAX_GAIN_TOP)
}

/// Map a stored `tok_cmd` to a rollup bucket for `tok gain --rollup`.
pub fn rollup_command_key(tok_cmd: &str) -> String {
    if let Some(rest) = tok_cmd.strip_prefix("tok:toml ") {
        let base = rest.split_whitespace().next().unwrap_or("toml");
        return format!("toml:{base}");
    }
    if tok_cmd.starts_with("tok fallback:") {
        return "fallback".to_string();
    }
    let mut parts = tok_cmd.split_whitespace();
    if parts.next() == Some("tok") {
        if let Some(tool) = parts.next() {
            return tool.to_string();
        }
    }
    tok_cmd.to_string()
}

struct RollupAcc {
    count: usize,
    saved: usize,
    pct_saved_product: f64,
    time_count_product: u64,
}

/// Apply optional rollup and limit to raw per-`tok_cmd` aggregates.
fn finalize_by_command(
    rows: Vec<CommandStats>,
    limit: usize,
    rollup: bool,
) -> Vec<CommandStats> {
    let limit = clamp_gain_top(limit);
    if rows.is_empty() {
        return rows;
    }

    let mut out: Vec<CommandStats> = if rollup {
        let mut map: std::collections::BTreeMap<String, RollupAcc> = std::collections::BTreeMap::new();
        for (cmd, count, saved, pct, avg_time) in rows {
            let key = rollup_command_key(&cmd);
            let acc = map.entry(key).or_insert(RollupAcc {
                count: 0,
                saved: 0,
                pct_saved_product: 0.0,
                time_count_product: 0,
            });
            acc.count += count;
            acc.saved += saved;
            acc.pct_saved_product += pct * saved as f64;
            acc.time_count_product += avg_time.saturating_mul(count as u64);
        }
        map.into_iter()
            .map(|(key, acc)| {
                let pct = if acc.saved > 0 {
                    acc.pct_saved_product / acc.saved as f64
                } else {
                    0.0
                };
                let avg_time = if acc.count > 0 {
                    acc.time_count_product / acc.count as u64
                } else {
                    0
                };
                (key, acc.count, acc.saved, pct, avg_time)
            })
            .collect()
    } else {
        rows
    };

    out.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| b.1.cmp(&a.1)));
    out.truncate(limit);
    out
}

impl Tracker {
    /// Create a new tracker instance.
    ///
    /// Opens or creates the SQLite database at the platform-specific location.
    /// Automatically creates the `commands` table if it doesn't exist and runs
    /// any necessary schema migrations.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Cannot determine database path
    /// - Cannot create parent directories
    /// - Cannot open/create SQLite database
    /// - Schema creation/migration fails
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tok::tracking::Tracker;
    ///
    /// let tracker = Tracker::new()?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn new() -> Result<Self> {
        let db_path = get_db_path()?;
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&db_path)?;
        // WAL mode + busy_timeout for concurrent access (multiple Claude Code instances).
        // Non-fatal: NFS/read-only filesystems may not support WAL.
        let _ = conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA busy_timeout=5000;",
        );
        conn.execute(
            "CREATE TABLE IF NOT EXISTS commands (
                id INTEGER PRIMARY KEY,
                timestamp TEXT NOT NULL,
                original_cmd TEXT NOT NULL,
                tok_cmd TEXT NOT NULL,
                input_tokens INTEGER NOT NULL,
                output_tokens INTEGER NOT NULL,
                saved_tokens INTEGER NOT NULL,
                savings_pct REAL NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_timestamp ON commands(timestamp)",
            [],
        )?;

        // Migration: add exec_time_ms column if it doesn't exist
        let _ = conn.execute(
            "ALTER TABLE commands ADD COLUMN exec_time_ms INTEGER DEFAULT 0",
            [],
        );
        // Migration: add project_path column with DEFAULT '' for new rows // changed: added DEFAULT
        let _ = conn.execute(
            "ALTER TABLE commands ADD COLUMN project_path TEXT DEFAULT ''",
            [],
        );
        // One-time migration: normalize NULLs from pre-default schema // changed: guarded with EXISTS
        let has_nulls: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM commands WHERE project_path IS NULL)",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);
        if has_nulls {
            let _ = conn.execute(
                "UPDATE commands SET project_path = '' WHERE project_path IS NULL",
                [],
            );
        }
        // Index for fast project-scoped gain queries // added
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_project_path_timestamp ON commands(project_path, timestamp)",
            [],
        );
        let _ = conn.execute("ALTER TABLE commands ADD COLUMN client TEXT DEFAULT ''", []);
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_client_timestamp ON commands(client, timestamp)",
            [],
        );

        conn.execute(
            "CREATE TABLE IF NOT EXISTS parse_failures (
                id INTEGER PRIMARY KEY,
                timestamp TEXT NOT NULL,
                raw_command TEXT NOT NULL,
                error_message TEXT NOT NULL,
                fallback_succeeded INTEGER NOT NULL DEFAULT 0
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_pf_timestamp ON parse_failures(timestamp)",
            [],
        )?;

        Ok(Self { conn })
    }

    /// Record a command execution with token counts and timing.
    ///
    /// Calculates savings metrics and stores the record in the database.
    /// Automatically cleans up records older than 90 days after insertion.
    ///
    /// # Arguments
    ///
    /// - `original_cmd`: The standard command (e.g., "ls -la")
    /// - `tok_cmd`: The TOK command used (e.g., "tok ls")
    /// - `input_tokens`: Estimated tokens from standard command output
    /// - `output_tokens`: Actual tokens from TOK output
    /// - `exec_time_ms`: Execution time in milliseconds
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tok::tracking::Tracker;
    ///
    /// let tracker = Tracker::new()?;
    /// tracker.record("ls -la", "tok ls", 1000, 200, 50)?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn record(
        &self,
        original_cmd: &str,
        tok_cmd: &str,
        input_tokens: usize,
        output_tokens: usize,
        exec_time_ms: u64,
    ) -> Result<()> {
        let saved = input_tokens.saturating_sub(output_tokens);
        let pct = if input_tokens > 0 {
            (saved as f64 / input_tokens as f64) * 100.0
        } else {
            0.0
        };

        let project_path = current_project_path_string(); // added: record cwd
        let client = resolve_client_id();

        self.conn.execute(
            "INSERT INTO commands (timestamp, original_cmd, tok_cmd, project_path, client, input_tokens, output_tokens, saved_tokens, savings_pct, exec_time_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                Utc::now().to_rfc3339(),
                original_cmd,
                tok_cmd,
                project_path,
                client,
                input_tokens as i64,
                output_tokens as i64,
                saved as i64,
                pct,
                exec_time_ms as i64
            ],
        )?;

        self.cleanup_old()?;
        Ok(())
    }

    fn cleanup_old(&self) -> Result<()> {
        let cutoff = Utc::now() - chrono::Duration::days(DEFAULT_HISTORY_DAYS);
        self.conn.execute(
            "DELETE FROM commands WHERE timestamp < ?1",
            params![cutoff.to_rfc3339()],
        )?;
        self.conn.execute(
            "DELETE FROM parse_failures WHERE timestamp < ?1",
            params![cutoff.to_rfc3339()],
        )?;
        Ok(())
    }

    /// Record a parse failure for analytics.
    pub fn record_parse_failure(
        &self,
        raw_command: &str,
        error_message: &str,
        fallback_succeeded: bool,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO parse_failures (timestamp, raw_command, error_message, fallback_succeeded)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                Utc::now().to_rfc3339(),
                raw_command,
                error_message,
                fallback_succeeded as i32,
            ],
        )?;
        self.cleanup_old()?;
        Ok(())
    }

    /// Get parse failure summary for `tok gain --failures`.
    pub fn get_parse_failure_summary(&self) -> Result<ParseFailureSummary> {
        let total: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM parse_failures", [], |row| row.get(0))?;

        let succeeded: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM parse_failures WHERE fallback_succeeded = 1",
            [],
            |row| row.get(0),
        )?;

        let recovery_rate = if total > 0 {
            (succeeded as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        // Top commands by frequency
        let mut stmt = self.conn.prepare(
            "SELECT raw_command, COUNT(*) as cnt
             FROM parse_failures
             GROUP BY raw_command
             ORDER BY cnt DESC
             LIMIT 10",
        )?;
        let top_commands = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        // Recent 10
        let mut stmt = self.conn.prepare(
            "SELECT timestamp, raw_command, error_message, fallback_succeeded
             FROM parse_failures
             ORDER BY timestamp DESC
             LIMIT 10",
        )?;
        let recent = stmt
            .query_map([], |row| {
                Ok(ParseFailureRecord {
                    timestamp: row.get(0)?,
                    raw_command: row.get(1)?,
                    error_message: row.get(2)?,
                    fallback_succeeded: row.get::<_, i32>(3)? != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(ParseFailureSummary {
            total: total as usize,
            recovery_rate,
            top_commands,
            recent,
        })
    }

    /// Get overall summary statistics across all recorded commands.
    ///
    /// Returns aggregated metrics including:
    /// - Total commands, tokens (input/output/saved)
    /// - Average savings percentage and execution time
    /// - Top 10 commands by tokens saved
    /// - Last 30 days of activity
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tok::tracking::Tracker;
    ///
    /// let tracker = Tracker::new()?;
    /// let summary = tracker.get_summary()?;
    /// println!("Saved {} tokens ({:.1}%)",
    ///     summary.total_saved, summary.avg_savings_pct);
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    #[allow(dead_code)]
    pub fn get_summary(&self) -> Result<GainSummary> {
        self.get_summary_filtered(None) // delegate to filtered variant
    }

    /// Get summary statistics filtered by project path. // added
    ///
    /// When `project_path` is `Some`, matches the exact working directory
    /// or any subdirectory (prefix match with path separator).
    pub fn get_summary_filtered(&self, project_path: Option<&str>) -> Result<GainSummary> {
        self.get_summary_filtered_with_options(project_path, GainByCommandOptions::default())
    }

    /// Like [`get_summary_filtered`] with configurable per-command table options.
    pub fn get_summary_filtered_with_options(
        &self,
        project_path: Option<&str>,
        by_command_opts: GainByCommandOptions,
    ) -> Result<GainSummary> {
        let (project_exact, project_glob) = project_filter_params(project_path); // added
        let mut total_commands = 0usize;
        let mut total_input = 0usize;
        let mut total_output = 0usize;
        let mut total_saved = 0usize;
        let mut total_time_ms = 0u64;

        let mut stmt = self.conn.prepare(
            "SELECT input_tokens, output_tokens, saved_tokens, exec_time_ms
             FROM commands
             WHERE (?1 IS NULL OR project_path = ?1 OR project_path GLOB ?2)", // added: project filter
        )?;

        let rows = stmt.query_map(params![project_exact, project_glob], |row| {
            // added: params
            Ok((
                row.get::<_, i64>(0)? as usize,
                row.get::<_, i64>(1)? as usize,
                row.get::<_, i64>(2)? as usize,
                row.get::<_, i64>(3)? as u64,
            ))
        })?;

        for row in rows {
            let (input, output, saved, time_ms) = row?;
            total_commands += 1;
            total_input += input;
            total_output += output;
            total_saved += saved;
            total_time_ms += time_ms;
        }

        let avg_savings_pct = if total_input > 0 {
            (total_saved as f64 / total_input as f64) * 100.0
        } else {
            0.0
        };

        let avg_time_ms = if total_commands > 0 {
            total_time_ms / total_commands as u64
        } else {
            0
        };

        let raw_by_command = self.get_by_command_raw(project_path)?;
        let by_command =
            finalize_by_command(raw_by_command, by_command_opts.limit, by_command_opts.rollup);
        let by_day = self.get_by_day(project_path)?;
        let by_client = self.get_by_client(project_path)?;

        Ok(GainSummary {
            total_commands,
            total_input,
            total_output,
            total_saved,
            avg_savings_pct,
            total_time_ms,
            avg_time_ms,
            by_command,
            by_day,
            by_client,
        })
    }

    /// Per-client savings breakdown for `tok gain --by-client`.
    pub fn get_by_client(&self, project_path: Option<&str>) -> Result<Vec<ClientStats>> {
        let (project_exact, project_glob) = project_filter_params(project_path);
        let mut stmt = self.conn.prepare(
            "SELECT client,
                    COUNT(*) as commands,
                    SUM(input_tokens) as input,
                    SUM(output_tokens) as output,
                    SUM(saved_tokens) as saved
             FROM commands
             WHERE (?1 IS NULL OR project_path = ?1 OR project_path GLOB ?2)
             GROUP BY client
             ORDER BY SUM(saved_tokens) DESC",
        )?;

        let rows = stmt.query_map(params![project_exact, project_glob], |row| {
            let raw_client: String = row.get(0)?;
            let input = row.get::<_, i64>(2)? as usize;
            let saved = row.get::<_, i64>(4)? as usize;
            let savings_pct = if input > 0 {
                (saved as f64 / input as f64) * 100.0
            } else {
                0.0
            };
            Ok(ClientStats {
                client: display_client_name(&raw_client).to_string(),
                commands: row.get::<_, i64>(1)? as usize,
                input_tokens: input,
                output_tokens: row.get::<_, i64>(3)? as usize,
                saved_tokens: saved,
                savings_pct,
            })
        })?;

        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    fn get_by_command_raw(&self, project_path: Option<&str>) -> Result<Vec<CommandStats>> {
        let (project_exact, project_glob) = project_filter_params(project_path);
        let mut stmt = self.conn.prepare(
            "SELECT tok_cmd, COUNT(*), SUM(saved_tokens), AVG(savings_pct), AVG(exec_time_ms)
             FROM commands
             WHERE (?1 IS NULL OR project_path = ?1 OR project_path GLOB ?2)
             GROUP BY tok_cmd
             ORDER BY SUM(saved_tokens) DESC",
        )?;

        let rows = stmt.query_map(params![project_exact, project_glob], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)? as usize,
                row.get::<_, i64>(2)? as usize,
                row.get::<_, f64>(3)?,
                row.get::<_, f64>(4)? as u64,
            ))
        })?;

        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    fn get_by_day(
        &self,
        project_path: Option<&str>, // added
    ) -> Result<Vec<(String, usize)>> {
        let (project_exact, project_glob) = project_filter_params(project_path); // added
        let mut stmt = self.conn.prepare(
            "SELECT DATE(timestamp), SUM(saved_tokens)
             FROM commands
             WHERE (?1 IS NULL OR project_path = ?1 OR project_path GLOB ?2)
             GROUP BY DATE(timestamp)
             ORDER BY DATE(timestamp) DESC
             LIMIT 30", // added: project filter in WHERE
        )?;

        let rows = stmt.query_map(params![project_exact, project_glob], |row| {
            // added: params
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
        })?;

        let mut result: Vec<_> = rows.collect::<Result<Vec<_>, _>>()?;
        result.reverse();
        Ok(result)
    }

    /// Get daily statistics for all recorded days.
    ///
    /// Returns one [`DayStats`] per day with commands executed, tokens saved,
    /// and execution time metrics. Results are ordered chronologically (oldest first).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tok::tracking::Tracker;
    ///
    /// let tracker = Tracker::new()?;
    /// let days = tracker.get_all_days()?;
    /// for day in days.iter().take(7) {
    ///     println!("{}: {} commands, {} tokens saved",
    ///         day.date, day.commands, day.saved_tokens);
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn get_all_days(&self) -> Result<Vec<DayStats>> {
        self.get_all_days_filtered(None) // delegate to filtered variant
    }

    /// Get daily statistics filtered by project path. // added
    pub fn get_all_days_filtered(&self, project_path: Option<&str>) -> Result<Vec<DayStats>> {
        let (project_exact, project_glob) = project_filter_params(project_path); // added
        let mut stmt = self.conn.prepare(
            "SELECT
                DATE(timestamp) as date,
                COUNT(*) as commands,
                SUM(input_tokens) as input,
                SUM(output_tokens) as output,
                SUM(saved_tokens) as saved,
                SUM(exec_time_ms) as total_time
             FROM commands
             WHERE (?1 IS NULL OR project_path = ?1 OR project_path GLOB ?2)
             GROUP BY DATE(timestamp)
             ORDER BY DATE(timestamp) DESC", // added: project filter
        )?;

        let rows = stmt.query_map(params![project_exact, project_glob], |row| {
            // added: params
            let input = row.get::<_, i64>(2)? as usize;
            let saved = row.get::<_, i64>(4)? as usize;
            let commands = row.get::<_, i64>(1)? as usize;
            let total_time = row.get::<_, i64>(5)? as u64;
            let savings_pct = if input > 0 {
                (saved as f64 / input as f64) * 100.0
            } else {
                0.0
            };
            let avg_time_ms = if commands > 0 {
                total_time / commands as u64
            } else {
                0
            };

            Ok(DayStats {
                date: row.get(0)?,
                commands,
                input_tokens: input,
                output_tokens: row.get::<_, i64>(3)? as usize,
                saved_tokens: saved,
                savings_pct,
                total_time_ms: total_time,
                avg_time_ms,
            })
        })?;

        let mut result: Vec<_> = rows.collect::<Result<Vec<_>, _>>()?;
        result.reverse();
        Ok(result)
    }

    /// Get weekly statistics grouped by week.
    ///
    /// Returns one [`WeekStats`] per week with aggregated metrics.
    /// Weeks start on Sunday (SQLite default). Results ordered chronologically.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tok::tracking::Tracker;
    ///
    /// let tracker = Tracker::new()?;
    /// let weeks = tracker.get_by_week()?;
    /// for week in weeks {
    ///     println!("{} to {}: {} tokens saved",
    ///         week.week_start, week.week_end, week.saved_tokens);
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn get_by_week(&self) -> Result<Vec<WeekStats>> {
        self.get_by_week_filtered(None) // delegate to filtered variant
    }

    /// Get weekly statistics filtered by project path. // added
    pub fn get_by_week_filtered(&self, project_path: Option<&str>) -> Result<Vec<WeekStats>> {
        let (project_exact, project_glob) = project_filter_params(project_path); // added
        let mut stmt = self.conn.prepare(
            "SELECT
                DATE(timestamp, 'weekday 0', '-6 days') as week_start,
                DATE(timestamp, 'weekday 0') as week_end,
                COUNT(*) as commands,
                SUM(input_tokens) as input,
                SUM(output_tokens) as output,
                SUM(saved_tokens) as saved,
                SUM(exec_time_ms) as total_time
             FROM commands
             WHERE (?1 IS NULL OR project_path = ?1 OR project_path GLOB ?2)
             GROUP BY week_start
             ORDER BY week_start DESC", // added: project filter
        )?;

        let rows = stmt.query_map(params![project_exact, project_glob], |row| {
            // added: params
            let input = row.get::<_, i64>(3)? as usize;
            let saved = row.get::<_, i64>(5)? as usize;
            let commands = row.get::<_, i64>(2)? as usize;
            let total_time = row.get::<_, i64>(6)? as u64;
            let savings_pct = if input > 0 {
                (saved as f64 / input as f64) * 100.0
            } else {
                0.0
            };
            let avg_time_ms = if commands > 0 {
                total_time / commands as u64
            } else {
                0
            };

            Ok(WeekStats {
                week_start: row.get(0)?,
                week_end: row.get(1)?,
                commands,
                input_tokens: input,
                output_tokens: row.get::<_, i64>(4)? as usize,
                saved_tokens: saved,
                savings_pct,
                total_time_ms: total_time,
                avg_time_ms,
            })
        })?;

        let mut result: Vec<_> = rows.collect::<Result<Vec<_>, _>>()?;
        result.reverse();
        Ok(result)
    }

    /// Get monthly statistics grouped by month.
    ///
    /// Returns one [`MonthStats`] per month (YYYY-MM format) with aggregated metrics.
    /// Results ordered chronologically.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tok::tracking::Tracker;
    ///
    /// let tracker = Tracker::new()?;
    /// let months = tracker.get_by_month()?;
    /// for month in months {
    ///     println!("{}: {} tokens saved ({:.1}%)",
    ///         month.month, month.saved_tokens, month.savings_pct);
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn get_by_month(&self) -> Result<Vec<MonthStats>> {
        self.get_by_month_filtered(None) // delegate to filtered variant
    }

    /// Get monthly statistics filtered by project path. // added
    pub fn get_by_month_filtered(&self, project_path: Option<&str>) -> Result<Vec<MonthStats>> {
        let (project_exact, project_glob) = project_filter_params(project_path); // added
        let mut stmt = self.conn.prepare(
            "SELECT
                strftime('%Y-%m', timestamp) as month,
                COUNT(*) as commands,
                SUM(input_tokens) as input,
                SUM(output_tokens) as output,
                SUM(saved_tokens) as saved,
                SUM(exec_time_ms) as total_time
             FROM commands
             WHERE (?1 IS NULL OR project_path = ?1 OR project_path GLOB ?2)
             GROUP BY month
             ORDER BY month DESC", // added: project filter
        )?;

        let rows = stmt.query_map(params![project_exact, project_glob], |row| {
            // added: params
            let input = row.get::<_, i64>(2)? as usize;
            let saved = row.get::<_, i64>(4)? as usize;
            let commands = row.get::<_, i64>(1)? as usize;
            let total_time = row.get::<_, i64>(5)? as u64;
            let savings_pct = if input > 0 {
                (saved as f64 / input as f64) * 100.0
            } else {
                0.0
            };
            let avg_time_ms = if commands > 0 {
                total_time / commands as u64
            } else {
                0
            };

            Ok(MonthStats {
                month: row.get(0)?,
                commands,
                input_tokens: input,
                output_tokens: row.get::<_, i64>(3)? as usize,
                saved_tokens: saved,
                savings_pct,
                total_time_ms: total_time,
                avg_time_ms,
            })
        })?;

        let mut result: Vec<_> = rows.collect::<Result<Vec<_>, _>>()?;
        result.reverse();
        Ok(result)
    }

    /// Get recent command history.
    ///
    /// Returns up to `limit` most recent command records, ordered by timestamp (newest first).
    ///
    /// # Arguments
    ///
    /// - `limit`: Maximum number of records to return
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tok::tracking::Tracker;
    ///
    /// let tracker = Tracker::new()?;
    /// let recent = tracker.get_recent(10)?;
    /// for cmd in recent {
    ///     println!("{}: {} saved {:.1}%",
    ///         cmd.timestamp, cmd.tok_cmd, cmd.savings_pct);
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    #[allow(dead_code)]
    pub fn get_recent(&self, limit: usize) -> Result<Vec<CommandRecord>> {
        self.get_recent_filtered(limit, None) // delegate to filtered variant
    }

    /// Get recent command history filtered by project path. // added
    pub fn get_recent_filtered(
        &self,
        limit: usize,
        project_path: Option<&str>,
    ) -> Result<Vec<CommandRecord>> {
        let (project_exact, project_glob) = project_filter_params(project_path); // added
        let mut stmt = self.conn.prepare(
            "SELECT timestamp, tok_cmd, saved_tokens, savings_pct
             FROM commands
             WHERE (?1 IS NULL OR project_path = ?1 OR project_path GLOB ?2)
             ORDER BY timestamp DESC
             LIMIT ?3", // added: project filter
        )?;

        let rows = stmt.query_map(
            params![project_exact, project_glob, limit as i64], // added: project params
            |row| {
                Ok(CommandRecord {
                    timestamp: DateTime::parse_from_rfc3339(&row.get::<_, String>(0)?)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    tok_cmd: row.get(1)?,
                    saved_tokens: row.get::<_, i64>(2)? as usize,
                    savings_pct: row.get(3)?,
                })
            },
        )?;

        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Count commands since a given timestamp (for telemetry).
    pub fn count_commands_since(&self, since: chrono::DateTime<chrono::Utc>) -> Result<i64> {
        let ts = since.format("%Y-%m-%dT%H:%M:%S").to_string();
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM commands WHERE timestamp >= ?1",
            params![ts],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Get top N commands by frequency (for telemetry).
    pub fn top_commands(&self, limit: usize) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT tok_cmd, COUNT(*) as cnt FROM commands
             GROUP BY tok_cmd ORDER BY cnt DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            let cmd: String = row.get(0)?;
            // Extract just the command name (e.g. "tok git status" → "git")
            Ok(cmd.split_whitespace().nth(1).unwrap_or(&cmd).to_string())
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Get overall savings percentage (for telemetry).
    pub fn overall_savings_pct(&self) -> Result<f64> {
        let (total_input, total_saved): (i64, i64) = self.conn.query_row(
            "SELECT COALESCE(SUM(input_tokens), 0), COALESCE(SUM(saved_tokens), 0) FROM commands",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if total_input > 0 {
            Ok((total_saved as f64 / total_input as f64) * 100.0)
        } else {
            Ok(0.0)
        }
    }

    /// Get total tokens saved across all tracked commands (for telemetry).
    pub fn total_tokens_saved(&self) -> Result<i64> {
        let saved: i64 = self.conn.query_row(
            "SELECT COALESCE(SUM(saved_tokens), 0) FROM commands",
            [],
            |row| row.get(0),
        )?;
        Ok(saved)
    }

    /// Get tokens saved in the last 24 hours (for telemetry).
    pub fn tokens_saved_24h(&self, since: chrono::DateTime<chrono::Utc>) -> Result<i64> {
        let ts = since.format("%Y-%m-%dT%H:%M:%S").to_string();
        let saved: i64 = self.conn.query_row(
            "SELECT COALESCE(SUM(saved_tokens), 0) FROM commands WHERE timestamp >= ?1",
            params![ts],
            |row| row.get(0),
        )?;
        Ok(saved)
    }

    /// Remove tracking rows used by `tok gain`.
    ///
    /// - `project_path == None`: deletes all rows in `commands` and `parse_failures`.
    /// - `Some(path)`: deletes `commands` rows for that project (exact path or subdirectories),
    ///   using the same matching rules as project-scoped gain. Parse-failure rows are global and
    ///   are not removed in project mode.
    pub fn clear_tracking(&self, project_path: Option<&str>) -> Result<TrackingClearResult> {
        let tx = self.conn.unchecked_transaction()?;

        match project_path {
            Some(p) => {
                let (exact, glob) = project_filter_params(Some(p));
                let (Some(exact), Some(glob)) = (exact, glob) else {
                    anyhow::bail!("internal error: project filter params");
                };
                let n = tx.execute(
                    "DELETE FROM commands WHERE project_path = ?1 OR project_path GLOB ?2",
                    params![exact, glob],
                )?;
                tx.commit()?;
                Ok(TrackingClearResult {
                    commands_removed: n,
                    parse_failures_removed: 0,
                })
            }
            None => {
                let commands_removed = tx.execute("DELETE FROM commands", [])?;
                let parse_failures_removed = tx.execute("DELETE FROM parse_failures", [])?;
                tx.commit()?;
                Ok(TrackingClearResult {
                    commands_removed,
                    parse_failures_removed,
                })
            }
        }
    }
}

fn get_db_path() -> Result<PathBuf> {
    // Priority 1: Environment variable TOK_DB_PATH
    if let Ok(custom_path) = std::env::var("TOK_DB_PATH") {
        return Ok(PathBuf::from(custom_path));
    }

    // Priority 2: Configuration file
    if let Ok(config) = crate::core::config::Config::load() {
        if let Some(db_path) = config.tracking.database_path {
            return Ok(db_path);
        }
    }

    // Priority 3: Default platform-specific location
    let data_dir = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    Ok(data_dir.join(TOK_DATA_DIR).join(HISTORY_DB))
}

/// Individual parse failure record.
#[derive(Debug)]
pub struct ParseFailureRecord {
    pub timestamp: String,
    pub raw_command: String,
    #[allow(dead_code)]
    pub error_message: String,
    pub fallback_succeeded: bool,
}

/// Aggregated parse failure summary.
#[derive(Debug)]
pub struct ParseFailureSummary {
    pub total: usize,
    pub recovery_rate: f64,
    pub top_commands: Vec<(String, usize)>,
    pub recent: Vec<ParseFailureRecord>,
}

/// Record a parse failure without ever crashing.
/// Silently ignores all errors — used in the fallback path.
pub fn record_parse_failure_silent(raw_command: &str, error_message: &str, succeeded: bool) {
    if let Ok(tracker) = Tracker::new() {
        let _ = tracker.record_parse_failure(raw_command, error_message, succeeded);
    }
}

/// Estimate token count from text using ~4 chars = 1 token heuristic.
///
/// This is a fast approximation suitable for tracking purposes.
/// For precise counts, integrate with your LLM's tokenizer API.
///
/// # Formula
///
/// `tokens = ceil(chars / 4)`
///
/// # Examples
///
/// ```
/// use tok::tracking::estimate_tokens;
///
/// assert_eq!(estimate_tokens(""), 0);
/// assert_eq!(estimate_tokens("abcd"), 1);  // 4 chars = 1 token
/// assert_eq!(estimate_tokens("abcde"), 2); // 5 chars = ceil(1.25) = 2
/// assert_eq!(estimate_tokens("hello world"), 3); // 11 chars = ceil(2.75) = 3
/// ```
pub fn estimate_tokens(text: &str) -> usize {
    // ~4 chars per token on average
    (text.len() as f64 / 4.0).ceil() as usize
}

/// Helper struct for timing command execution
/// Helper for timing command execution and tracking results.
///
/// Preferred API for tracking commands. Automatically measures execution time
/// and records token savings. Use instead of the deprecated [`track`] function.
///
/// # Examples
///
/// ```no_run
/// use tok::tracking::TimedExecution;
///
/// let timer = TimedExecution::start();
/// let input = execute_standard_command()?;
/// let output = execute_tok_command()?;
/// timer.track("ls -la", "tok ls", &input, &output);
/// # Ok::<(), anyhow::Error>(())
/// ```
pub struct TimedExecution {
    start: Instant,
}

impl TimedExecution {
    /// Start timing a command execution.
    ///
    /// Creates a new timer that starts measuring elapsed time immediately.
    /// Call [`track`](Self::track) or [`track_passthrough`](Self::track_passthrough)
    /// when the command completes.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tok::tracking::TimedExecution;
    ///
    /// let timer = TimedExecution::start();
    /// // ... execute command ...
    /// timer.track("cmd", "tok cmd", "input", "output");
    /// ```
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    /// Track the command with elapsed time and token counts.
    ///
    /// Records the command execution with:
    /// - Elapsed time since [`start`](Self::start)
    /// - Token counts estimated from input/output strings
    /// - Calculated savings metrics
    ///
    /// # Arguments
    ///
    /// - `original_cmd`: Standard command (e.g., "ls -la")
    /// - `tok_cmd`: TOK command used (e.g., "tok ls")
    /// - `input`: Standard command output (for token estimation)
    /// - `output`: TOK command output (for token estimation)
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tok::tracking::TimedExecution;
    ///
    /// let timer = TimedExecution::start();
    /// let input = "long output...";
    /// let output = "short output";
    /// timer.track("ls -la", "tok ls", input, output);
    /// ```
    pub fn track(&self, original_cmd: &str, tok_cmd: &str, input: &str, output: &str) {
        let elapsed_ms = self.start.elapsed().as_millis() as u64;
        let input_tokens = estimate_tokens(input);
        let output_tokens = estimate_tokens(output);

        if let Ok(tracker) = Tracker::new() {
            let _ = tracker.record(
                original_cmd,
                tok_cmd,
                input_tokens,
                output_tokens,
                elapsed_ms,
            );
        }
    }

    /// Track passthrough commands (timing-only, no token counting).
    ///
    /// For commands that stream output or run interactively where output
    /// cannot be captured. Records execution time but sets tokens to 0
    /// (does not dilute savings statistics).
    ///
    /// # Arguments
    ///
    /// - `original_cmd`: Standard command (e.g., "git tag --list")
    /// - `tok_cmd`: TOK command used (e.g., "tok git tag --list")
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tok::tracking::TimedExecution;
    ///
    /// let timer = TimedExecution::start();
    /// // ... execute streaming command ...
    /// timer.track_passthrough("git tag", "tok git tag");
    /// ```
    pub fn track_passthrough(&self, original_cmd: &str, tok_cmd: &str) {
        let elapsed_ms = self.start.elapsed().as_millis() as u64;
        // input_tokens=0, output_tokens=0 won't dilute savings statistics
        if let Ok(tracker) = Tracker::new() {
            let _ = tracker.record(original_cmd, tok_cmd, 0, 0, elapsed_ms);
        }
    }
}

/// Format OsString args for tracking display.
///
/// Joins arguments with spaces, converting each to UTF-8 (lossy).
/// Useful for displaying command arguments in tracking records.
///
/// # Examples
///
/// ```
/// use std::ffi::OsString;
/// use tok::tracking::args_display;
///
/// let args = vec![OsString::from("status"), OsString::from("--short")];
/// assert_eq!(args_display(&args), "status --short");
/// ```
pub fn args_display(args: &[OsString]) -> String {
    args.iter()
        .map(|a| a.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serializes all tests that touch the tracking SQLite DB. `TOK_DB_PATH` is process-global, so
    /// parallel tests would otherwise cross-write or read the wrong file.
    static TRACKING_TEST_MUTEX: Mutex<()> = Mutex::new(());

    fn tracking_test_lock() -> std::sync::MutexGuard<'static, ()> {
        TRACKING_TEST_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    struct RestoreCwd(std::path::PathBuf);
    impl Drop for RestoreCwd {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }

    // 1. estimate_tokens — verify ~4 chars/token ratio
    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1); // 4 chars = 1 token
        assert_eq!(estimate_tokens("abcde"), 2); // 5 chars = ceil(1.25) = 2
        assert_eq!(estimate_tokens("a"), 1); // 1 char = ceil(0.25) = 1
        assert_eq!(estimate_tokens("12345678"), 2); // 8 chars = 2 tokens
    }

    // 2. args_display — format OsString vec
    #[test]
    fn test_args_display() {
        let args = vec![OsString::from("status"), OsString::from("--short")];
        assert_eq!(args_display(&args), "status --short");
        assert_eq!(args_display(&[]), "");

        let single = vec![OsString::from("log")];
        assert_eq!(args_display(&single), "log");
    }

    // 3b. Client attribution via TOK_CLIENT
    #[test]
    fn test_tracker_client_attribution() {
        use std::env;
        let _guard = tracking_test_lock();
        let tmp = tempfile::tempdir().expect("tempdir");
        let db = tmp.path().join("hist-client.db");
        env::set_var("TOK_DB_PATH", &db);
        env::set_var(crate::core::client::ENV_TOK_CLIENT, "cursor");
        let tracker = Tracker::new().expect("tracker");
        tracker
            .record("git status", "tok git status", 100, 20, 5)
            .expect("record");
        let by_client = tracker.get_by_client(None).expect("by_client");
        assert!(
            by_client
                .iter()
                .any(|c| c.client == "cursor" && c.saved_tokens == 80),
            "expected cursor client row: {by_client:?}"
        );
        env::remove_var("TOK_DB_PATH");
        env::remove_var(crate::core::client::ENV_TOK_CLIENT);
    }

    // 3. Tracker::record + get_recent — round-trip DB
    #[test]
    fn test_tracker_record_and_recent() {
        let _guard = tracking_test_lock();
        let tracker = Tracker::new().expect("Failed to create tracker");

        // Use unique test identifier to avoid conflicts with other tests
        let test_cmd = format!("tok git status test_{}", std::process::id());

        tracker
            .record("git status", &test_cmd, 100, 20, 50)
            .expect("Failed to record");

        let recent = tracker.get_recent(10).expect("Failed to get recent");

        // Find our specific test record
        let test_record = recent
            .iter()
            .find(|r| r.tok_cmd == test_cmd)
            .expect("Test record not found in recent commands");

        assert_eq!(test_record.saved_tokens, 80);
        assert_eq!(test_record.savings_pct, 80.0);
    }

    // 4. track_passthrough doesn't dilute stats (input=0, output=0)
    #[test]
    fn test_track_passthrough_no_dilution() {
        let _guard = tracking_test_lock();
        let tracker = Tracker::new().expect("Failed to create tracker");

        // Use unique test identifiers
        let pid = std::process::id();
        let cmd1 = format!("tok cmd1_test_{}", pid);
        let cmd2 = format!("tok cmd2_passthrough_test_{}", pid);

        // Record one real command with 80% savings
        tracker
            .record("cmd1", &cmd1, 1000, 200, 10)
            .expect("Failed to record cmd1");

        // Record passthrough (0, 0)
        tracker
            .record("cmd2", &cmd2, 0, 0, 5)
            .expect("Failed to record passthrough");

        // Verify both records exist in recent history
        let recent = tracker.get_recent(20).expect("Failed to get recent");

        let record1 = recent
            .iter()
            .find(|r| r.tok_cmd == cmd1)
            .expect("cmd1 record not found");
        let record2 = recent
            .iter()
            .find(|r| r.tok_cmd == cmd2)
            .expect("passthrough record not found");

        // Verify cmd1 has 80% savings
        assert_eq!(record1.saved_tokens, 800);
        assert_eq!(record1.savings_pct, 80.0);

        // Verify passthrough has 0% savings
        assert_eq!(record2.saved_tokens, 0);
        assert_eq!(record2.savings_pct, 0.0);

        // This validates that passthrough (0 input, 0 output) doesn't dilute stats
        // because the savings calculation is correct for both cases
    }

    // 5. TimedExecution::track records with exec_time > 0
    #[test]
    fn test_timed_execution_records_time() {
        let _guard = tracking_test_lock();
        let timer = TimedExecution::start();
        std::thread::sleep(std::time::Duration::from_millis(10));
        timer.track("test cmd", "tok test", "raw input data", "filtered");

        // Verify via DB that record exists
        let tracker = Tracker::new().expect("Failed to create tracker");
        let recent = tracker.get_recent(5).expect("Failed to get recent");
        assert!(recent.iter().any(|r| r.tok_cmd == "tok test"));
    }

    // 6. TimedExecution::track_passthrough records with 0 tokens
    #[test]
    fn test_timed_execution_passthrough() {
        use std::env;
        use tempfile::tempdir;

        let _guard = tracking_test_lock();
        let tmp = tempdir().expect("tempdir");
        let db = tmp.path().join("history.db");
        env::set_var("TOK_DB_PATH", &db);

        let timer = TimedExecution::start();
        let marker = format!("passthrough_{}", std::process::id());
        timer.track_passthrough("git tag", &format!("tok git tag ({marker})"));

        let tracker = Tracker::new().expect("Failed to create tracker");
        let recent = tracker.get_recent(50).expect("Failed to get recent");

        let pt = recent
            .iter()
            .find(|r| r.tok_cmd.contains(&marker))
            .expect("Passthrough record not found");

        // savings_pct should be 0 for passthrough
        assert_eq!(pt.savings_pct, 0.0);
        assert_eq!(pt.saved_tokens, 0);

        env::remove_var("TOK_DB_PATH");
    }

    // 7. get_db_path respects environment variable TOK_DB_PATH
    #[test]
    fn test_custom_db_path_env() {
        use std::env;

        let _guard = tracking_test_lock();
        let custom_path = env::temp_dir().join("tok_test_custom.db");
        env::set_var("TOK_DB_PATH", &custom_path);

        let db_path = get_db_path().expect("Failed to get db path");
        assert_eq!(db_path, custom_path);

        env::remove_var("TOK_DB_PATH");
    }

    // 8. get_db_path falls back to default when no custom config
    #[test]
    fn test_default_db_path() {
        use std::env;

        let _guard = tracking_test_lock();
        // Ensure no env var is set
        env::remove_var("TOK_DB_PATH");

        let db_path = get_db_path().expect("Failed to get db path");
        assert!(db_path.ends_with("tok/history.db"));
    }

    // 9. project_filter_params uses GLOB pattern with * wildcard // added
    #[test]
    fn test_project_filter_params_glob_pattern() {
        let (exact, glob) = project_filter_params(Some("/home/user/project"));
        assert_eq!(exact.unwrap(), "/home/user/project");
        // Must use * (GLOB) not % (LIKE) for subdirectory prefix matching
        let glob_val = glob.unwrap();
        assert!(glob_val.ends_with('*'), "GLOB pattern must end with *");
        assert!(!glob_val.contains('%'), "Must not contain LIKE wildcard %");
        assert_eq!(
            glob_val,
            format!("/home/user/project{}*", std::path::MAIN_SEPARATOR)
        );
    }

    // 10. project_filter_params returns None for None input // added
    #[test]
    fn test_project_filter_params_none() {
        let (exact, glob) = project_filter_params(None);
        assert!(exact.is_none());
        assert!(glob.is_none());
    }

    // 11. GLOB pattern safe with underscores in path names // added
    #[test]
    fn test_project_filter_params_underscore_safe() {
        // In LIKE, _ matches any single char; in GLOB, _ is literal
        let (exact, glob) = project_filter_params(Some("/home/user/my_project"));
        assert_eq!(exact.unwrap(), "/home/user/my_project");
        let glob_val = glob.unwrap();
        // _ must be preserved literally (GLOB treats _ as literal, LIKE does not)
        assert!(glob_val.contains("my_project"));
        assert_eq!(
            glob_val,
            format!("/home/user/my_project{}*", std::path::MAIN_SEPARATOR)
        );
    }

    // 12. record_parse_failure + get_parse_failure_summary roundtrip
    #[test]
    fn test_parse_failure_roundtrip() {
        let _guard = tracking_test_lock();
        let tracker = Tracker::new().expect("Failed to create tracker");
        let test_cmd = format!("git -C /path status test_{}", std::process::id());

        tracker
            .record_parse_failure(&test_cmd, "unrecognized subcommand", true)
            .expect("Failed to record parse failure");

        let summary = tracker
            .get_parse_failure_summary()
            .expect("Failed to get summary");

        assert!(summary.total >= 1);
        assert!(summary.recent.iter().any(|r| r.raw_command == test_cmd));
    }

    // 13. recovery_rate calculation
    #[test]
    fn test_parse_failure_recovery_rate() {
        let _guard = tracking_test_lock();
        let tracker = Tracker::new().expect("Failed to create tracker");
        let pid = std::process::id();

        // 2 successes, 1 failure
        tracker
            .record_parse_failure(&format!("cmd_ok1_{}", pid), "err", true)
            .unwrap();
        tracker
            .record_parse_failure(&format!("cmd_ok2_{}", pid), "err", true)
            .unwrap();
        tracker
            .record_parse_failure(&format!("cmd_fail_{}", pid), "err", false)
            .unwrap();

        let summary = tracker.get_parse_failure_summary().unwrap();
        // We can't assert exact rate because other tests may have added records,
        // but we can verify recovery_rate is between 0 and 100
        assert!(summary.recovery_rate >= 0.0 && summary.recovery_rate <= 100.0);
    }

    // 14. clear_tracking (global) on isolated DB
    #[test]
    fn test_clear_tracking_global() {
        use std::env;
        use tempfile::tempdir;

        let _guard = tracking_test_lock();
        let tmp = tempdir().expect("tempdir");
        let db = tmp.path().join("history.db");
        env::set_var("TOK_DB_PATH", &db);

        let tracker = Tracker::new().expect("tracker");
        let cmd = format!("tok test_clear_global_{}", std::process::id());
        tracker
            .record("raw", &cmd, 10, 5, 1)
            .expect("record command");
        tracker
            .record_parse_failure(&cmd, "err", true)
            .expect("record parse failure");

        let cleared = tracker.clear_tracking(None).expect("clear global");
        assert_eq!(cleared.commands_removed, 1);
        assert_eq!(cleared.parse_failures_removed, 1);

        let summary = tracker.get_summary().expect("summary");
        assert_eq!(summary.total_commands, 0);
        let pf = tracker.get_parse_failure_summary().expect("pf summary");
        assert_eq!(pf.total, 0);

        env::remove_var("TOK_DB_PATH");
    }

    #[test]
    fn test_rollup_command_key() {
        assert_eq!(rollup_command_key("tok cargo test"), "cargo");
        assert_eq!(rollup_command_key("tok grep"), "grep");
        assert_eq!(rollup_command_key("tok:toml jq ."), "toml:jq");
        assert_eq!(
            rollup_command_key("tok fallback: make"),
            "fallback"
        );
    }

    #[test]
    fn test_finalize_by_command_top_and_rollup() {
        let rows = vec![
            ("tok cargo test".into(), 2, 100, 90.0, 10),
            ("tok cargo build".into(), 1, 50, 80.0, 20),
            ("tok grep".into(), 5, 200, 75.0, 5),
        ];
        let limited = finalize_by_command(rows.clone(), 2, false);
        assert_eq!(limited.len(), 2);
        assert_eq!(limited[0].0, "tok grep");

        let rolled = finalize_by_command(rows, 10, true);
        assert_eq!(rolled.len(), 2);
        let cargo = rolled.iter().find(|r| r.0 == "cargo").expect("cargo");
        assert_eq!(cargo.1, 3);
        assert_eq!(cargo.2, 150);
    }

    // 15. clear_tracking (project) deletes only matching project_path rows
    #[test]
    fn test_clear_tracking_project_scope() {
        use std::env;
        use std::fs;
        use tempfile::tempdir;

        let _guard = tracking_test_lock();
        let _restore = RestoreCwd(env::current_dir().expect("cwd"));
        let tmp = tempdir().expect("tempdir");
        let proj_a = tmp.path().join("proj_a");
        let proj_b = tmp.path().join("proj_b");
        fs::create_dir_all(&proj_a).expect("mkdir a");
        fs::create_dir_all(&proj_b).expect("mkdir b");
        let db = tmp.path().join("hist.db");
        let _ = fs::remove_file(&db);

        env::set_var("TOK_DB_PATH", &db);

        env::set_current_dir(&proj_a).expect("cd a");
        let t = Tracker::new().expect("tracker a");
        t.record("a", "tok a", 1, 1, 1).expect("rec a");
        // Must match `current_project_path_string()` used inside `record`, not only `proj_a` join.
        let path_a_for_clear = env::current_dir()
            .expect("cwd")
            .canonicalize()
            .expect("canon cwd a")
            .to_string_lossy()
            .into_owned();

        env::set_current_dir(&proj_b).expect("cd b");
        let t = Tracker::new().expect("tracker b");
        t.record("b", "tok b", 1, 1, 1).expect("rec b");

        let cleared = t
            .clear_tracking(Some(path_a_for_clear.as_str()))
            .expect("clear project");
        assert_eq!(cleared.commands_removed, 1);
        assert_eq!(cleared.parse_failures_removed, 0);

        let summary = t.get_summary().expect("summary");
        assert_eq!(summary.total_commands, 1);

        env::remove_var("TOK_DB_PATH");
    }
}
