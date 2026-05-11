mod analytics;
mod cli_dispatch;
mod cmds;
mod core;
mod discover;
mod forgemap;
mod hooks;
mod learn;
mod mem;
mod parser;
#[allow(dead_code)]
mod security;

// `crate::…` paths in `src/cmds/**` expect these at the crate root.
pub(crate) use cmds::dotnet::{binlog, dotnet_format_report, dotnet_trx};
pub(crate) use cmds::git::git;
pub(crate) use cmds::go::golangci_cmd;
pub(crate) use cmds::js::prettier_cmd;
pub(crate) use cmds::python::{mypy_cmd, ruff_cmd};
pub(crate) use cmds::system::{json_cmd, log_cmd};

use anyhow::Result;
use clap::error::ErrorKind;
use clap::{Parser, Subcommand, ValueEnum};
use colored::Colorize;
use std::ffi::OsString;
use std::io::IsTerminal;
use std::path::PathBuf;

/// Which coding agent gets the hook treatment.
#[derive(Debug, Clone, Copy, PartialEq, ValueEnum)]
pub enum AgentTarget {
    /// Claude Code (the usual default)
    Claude,
    /// Cursor — editor + CLI agent
    Cursor,
    /// Windsurf / Cascade
    Windsurf,
    /// Cline or Roo Code in VS Code
    Cline,
}

#[derive(Parser)]
#[command(
    name = "tok",
    version,
    about = "Token Optimization Kit — squeeze noisy CLI output before it hits your LLM",
    long_about = "One small Rust binary that sits between your tools and your model: it filters, groups, and truncates command output so you keep the signal and burn fewer tokens."
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// How chatty tok should be (-v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    /// Extra-cramped mode: ASCII icons, inline fields — max squeeze
    #[arg(short = 'u', long, global = true)]
    ultra_compact: bool,

    /// Pass SKIP_ENV_VALIDATION=1 to children (Next.js, tsc, linters, Prisma, …)
    #[arg(long = "skip-env", global = true)]
    skip_env: bool,

    /// Enable security/privacy layer (obfuscate sensitive data)
    #[arg(long = "security", global = true)]
    security: bool,

    /// Disable security/privacy layer (overrides config)
    #[arg(long = "no-security", global = true, conflicts_with = "security")]
    no_security: bool,

    /// Security mode: observe, balanced, strict, developer
    #[arg(long = "security-mode", global = true)]
    security_mode: Option<String>,

    /// Enable local SLM for semantic security scanning
    #[arg(long = "slm", global = true)]
    slm: bool,

    /// Disable local SLM (overrides config)
    #[arg(long = "no-slm", global = true, conflicts_with = "slm")]
    no_slm: bool,
}

#[derive(Subcommand)]
pub(crate) enum Commands {
    /// ls, but your LLM doesn’t need every column in the universe
    Ls {
        /// Arguments passed to ls (supports all native ls flags like -l, -a, -h, -R)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// tree(1) you can actually scroll past
    Tree {
        /// Arguments passed to tree (supports all native tree flags like -L, -d, -a)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Read files — smart filters trim the fluff
    Read {
        /// Files to read (supports multiple, like cat)
        #[arg(required = true, num_args = 1..)]
        files: Vec<PathBuf>,
        /// Filter: none (default, full content), minimal, aggressive
        #[arg(short, long, default_value = "none")]
        level: core::filter::FilterLevel,
        /// Max lines
        #[arg(short, long, conflicts_with = "tail_lines")]
        max_lines: Option<usize>,
        /// Keep only last N lines
        #[arg(long, conflicts_with = "max_lines")]
        tail_lines: Option<usize>,
        /// Show line numbers
        #[arg(short = 'n', long)]
        line_numbers: bool,
    },

    /// Two-line “what even is this file?” (heuristic, no cloud calls)
    Smart {
        /// File to analyze
        file: PathBuf,
        /// Model: heuristic
        #[arg(short, long, default_value = "heuristic")]
        model: String,
        /// Force model download
        #[arg(long)]
        force_download: bool,
    },

    /// git without the wall of text
    Git {
        /// Change to directory before executing (like git -C <path>, can be repeated)
        #[arg(short = 'C', action = clap::ArgAction::Append)]
        directory: Vec<String>,

        /// Git configuration override (like git -c key=value, can be repeated)
        #[arg(short = 'c', action = clap::ArgAction::Append)]
        config_override: Vec<String>,

        /// Set the path to the .git directory
        #[arg(long = "git-dir")]
        git_dir: Option<String>,

        /// Set the path to the working tree
        #[arg(long = "work-tree")]
        work_tree: Option<String>,

        /// Disable pager (like git --no-pager)
        #[arg(long = "no-pager")]
        no_pager: bool,

        /// Skip optional locks (like git --no-optional-locks)
        #[arg(long = "no-optional-locks")]
        no_optional_locks: bool,

        /// Treat repository as bare (like git --bare)
        #[arg(long)]
        bare: bool,

        /// Treat pathspecs literally (like git --literal-pathspecs)
        #[arg(long = "literal-pathspecs")]
        literal_pathspecs: bool,

        #[command(subcommand)]
        command: GitCommands,
    },

    /// GitHub CLI — PRs, issues, runs, less noise
    Gh {
        /// Subcommand: pr, issue, run, repo
        subcommand: String,
        /// Additional arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// AWS CLI — JSON in, human-sized lines out
    Aws {
        /// AWS service subcommand (e.g., sts, s3, ec2, ecs, rds, cloudformation)
        subcommand: String,
        /// Additional arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// psql — tidy tables, fewer borders, fewer tokens
    #[command(disable_help_flag = true)]
    Psql {
        /// psql arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// pnpm on “quiet room” mode
    Pnpm {
        #[command(subcommand)]
        command: PnpmCommands,
    },

    /// Run anything — print only the spicy bits (errors & warnings)
    Err {
        /// Command to run
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },

    /// Run tests and show only failures
    Test {
        /// Test command (e.g. cargo test)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },

    /// JSON — shrink values or show shape-only with --schema
    Json {
        /// JSON file
        file: PathBuf,
        /// Max depth
        #[arg(short, long, default_value = "5")]
        depth: usize,
        /// Show structure only (strip all values)
        #[arg(long)]
        schema: bool,
    },

    /// Dependencies without the manifest novel
    Deps {
        /// Project path
        #[arg(default_value = ".")]
        path: PathBuf,
    },

    /// Env vars — filtered, secrets stay shy
    Env {
        /// Filter by name (e.g. PATH, AWS)
        #[arg(short, long)]
        filter: Option<String>,
        /// Show all (include sensitive)
        #[arg(long)]
        show_all: bool,
    },

    /// find — compact tree-ish output (native flags welcome)
    Find {
        /// All find arguments (supports both TOK and native find syntax)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Diff — only the lines that actually moved
    Diff {
        /// First file or - for stdin (unified diff)
        file1: PathBuf,
        /// Second file (optional if stdin)
        file2: Option<PathBuf>,
    },

    /// Logs — dedupe repeats, keep the story
    Log {
        /// Log file (omit for stdin)
        file: Option<PathBuf>,
    },

    /// .NET — build/test/restore/format without the scroll marathon
    Dotnet {
        #[command(subcommand)]
        command: DotnetCommands,
    },

    /// Docker — lists and logs that fit in context
    Docker {
        #[command(subcommand)]
        command: DockerCommands,
    },

    /// kubectl — pods, logs, services, fewer walls of YAML
    Kubectl {
        #[command(subcommand)]
        command: KubectlCommands,
    },

    /// Run a command, get a cheeky heuristic summary
    Summary {
        /// Command to run and summarize
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },

    /// grep/rg — grouped, trimmed, file-aware
    Grep {
        /// Pattern to search
        pattern: String,
        /// Path to search in
        #[arg(default_value = ".")]
        path: String,
        /// Max line length
        #[arg(short = 'l', long, default_value = "80")]
        max_len: usize,
        /// Max results to show
        #[arg(short, long, default_value = "200")]
        max: usize,
        /// Show only match context (not full line)
        #[arg(short, long)]
        context_only: bool,
        /// Filter by file type (e.g., ts, py, rust)
        #[arg(short = 't', long)]
        file_type: Option<String>,
        /// Show line numbers (always on, accepted for grep/rg compatibility)
        #[arg(short = 'n', long)]
        line_numbers: bool,
        /// Extra ripgrep arguments (e.g., -i, -A 3, -w, --glob)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        extra_args: Vec<String>,
    },

    /// Wire tok into your agent (hooks, TOK.md, the whole vibe)
    Init {
        /// Add to global assistant config directory instead of local project file
        #[arg(short, long)]
        global: bool,

        /// Install OpenCode plugin (in addition to Claude Code)
        #[arg(long)]
        opencode: bool,

        /// Initialize for Gemini CLI instead of Claude Code
        #[arg(long)]
        gemini: bool,

        /// Target agent to install hooks for (default: claude)
        #[arg(long, value_enum)]
        agent: Option<AgentTarget>,

        /// Show current configuration
        #[arg(long)]
        show: bool,

        /// Inject full instructions into CLAUDE.md (legacy mode)
        #[arg(long = "claude-md", group = "mode")]
        claude_md: bool,

        /// Hook only, no TOK.md
        #[arg(long = "hook-only", group = "mode")]
        hook_only: bool,

        /// Auto-patch settings.json without prompting
        #[arg(long = "auto-patch", group = "patch")]
        auto_patch: bool,

        /// Skip settings.json patching (print manual instructions)
        #[arg(long = "no-patch", group = "patch")]
        no_patch: bool,

        /// Remove TOK artifacts for the selected assistant mode
        #[arg(long)]
        uninstall: bool,

        /// Target Codex CLI (uses AGENTS.md + TOK.md, no Claude hook patching)
        #[arg(long)]
        codex: bool,

        /// Install GitHub Copilot integration (VS Code + CLI)
        #[arg(long)]
        copilot: bool,

        /// Install for ALL supported agents at once (global + project-local)
        #[arg(long)]
        all: bool,
    },

    /// wget — skip the progress-bar light show
    Wget {
        /// URL to download
        url: String,
        /// Output file (-O - for stdout)
        #[arg(short = 'O', long = "output-document", allow_hyphen_values = true)]
        output: Option<String>,
        /// Additional wget arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// wc — counts without decorative padding
    Wc {
        /// Arguments passed to wc (files, flags like -l, -w, -c)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Token savings stats — flex your `tok gain` numbers
    Gain {
        /// Filter statistics to current project (current working directory) // added
        #[arg(short, long)]
        project: bool,
        /// Show ASCII graph of daily savings
        #[arg(short, long)]
        graph: bool,
        /// Show recent command history
        #[arg(short = 'H', long)]
        history: bool,
        /// Show monthly quota savings estimate
        #[arg(short, long)]
        quota: bool,
        /// Subscription tier for quota calculation: pro, 5x, 20x
        #[arg(short, long, default_value = "20x", requires = "quota")]
        tier: String,
        /// Show detailed daily breakdown (all days)
        #[arg(short, long)]
        daily: bool,
        /// Show weekly breakdown
        #[arg(short, long)]
        weekly: bool,
        /// Show monthly breakdown
        #[arg(short, long)]
        monthly: bool,
        /// Show all time breakdowns (daily + weekly + monthly)
        #[arg(short, long)]
        all: bool,
        /// Output format: text, json, csv
        #[arg(short, long, default_value = "text")]
        format: String,
        /// Show parse failure log (commands that fell back to raw execution)
        #[arg(short = 'F', long)]
        failures: bool,
        /// Clear stored gain / tracking data (`--project` limits to the current directory tree)
        #[arg(long)]
        reset: bool,
    },

    /// Claude spend vs tok savings — receipts included
    CcEconomics {
        /// Show detailed daily breakdown
        #[arg(short, long)]
        daily: bool,
        /// Show weekly breakdown
        #[arg(short, long)]
        weekly: bool,
        /// Show monthly breakdown
        #[arg(short, long)]
        monthly: bool,
        /// Show all time breakdowns (daily + weekly + monthly)
        #[arg(short, long)]
        all: bool,
        /// Output format: text, json, csv
        #[arg(short, long, default_value = "text")]
        format: String,
    },

    /// Peek or scaffold your tok config
    Config {
        /// Create default config file
        #[arg(long)]
        create: bool,
    },

    /// Vitest — failures loud, noise quiet
    Vitest {
        #[command(subcommand)]
        command: VitestCommands,
    },

    /// Prisma — no ASCII art victory laps
    Prisma {
        #[command(subcommand)]
        command: PrismaCommands,
    },

    /// tsc — errors grouped so you fix, not scroll
    Tsc {
        /// TypeScript compiler arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// next build — same result, fewer tokens
    Next {
        /// Next.js build arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// ESLint — violations grouped by rule/file
    Lint {
        /// Linter arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Prettier check — who needs formatting, fast
    Prettier {
        /// Prettier arguments (e.g., --check, --write)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Format check — auto-picks prettier / black / ruff format
    Format {
        /// Formatter arguments (auto-detects formatter from project files)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Playwright — E2E results without the novel
    Playwright {
        /// Playwright arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// cargo — builds, tests, clippy, less scroll
    Cargo {
        #[command(subcommand)]
        command: CargoCommands,
    },

    /// npm run — boilerplate stripped, signal kept
    Npm {
        /// npm run arguments (script name + options)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// npx — smart routing to tsc/eslint/prisma filters when it can
    Npx {
        /// npx arguments (command + options)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// curl — JSON detected; schema mode when you want shapes only
    Curl {
        /// Curl arguments (URL + options)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Mine Claude history for “you could’ve tok’d that” moments
    Discover {
        /// Filter by project path (substring match)
        #[arg(short, long)]
        project: Option<String>,
        /// Max commands per section
        #[arg(short, long, default_value = "15")]
        limit: usize,
        /// Scan all projects (default: current project only)
        #[arg(short, long)]
        all: bool,
        /// Limit to sessions from last N days
        #[arg(short, long, default_value = "30")]
        since: u64,
        /// Output format: text, json
        #[arg(short, long, default_value = "text")]
        format: String,
    },

    /// How hard you’re actually leaning on tok across sessions
    Session {},

    /// Learn CLI fixes from past Claude face-plants
    Learn {
        /// Filter by project path (substring match)
        #[arg(short, long)]
        project: Option<String>,
        /// Scan all projects (default: current project only)
        #[arg(short, long)]
        all: bool,
        /// Limit to sessions from last N days
        #[arg(short, long, default_value = "30")]
        since: u64,
        /// Output format: text, json
        #[arg(short, long, default_value = "text")]
        format: String,
        /// Generate .claude/rules/cli-corrections.md file
        #[arg(short, long)]
        write_rules: bool,
        /// Minimum confidence threshold (0.0-1.0)
        #[arg(long, default_value = "0.6")]
        min_confidence: f64,
        /// Minimum occurrences to include in report
        #[arg(long, default_value = "1")]
        min_occurrences: usize,
    },

    /// Raw command passthrough — still counts toward your stats
    Proxy {
        /// Command and arguments to execute
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },

    /// Trust this project’s .tok filter recipes
    Trust {
        /// List all trusted projects
        #[arg(long)]
        list: bool,
    },

    /// Nope out of trusted local TOML filters
    Untrust,

    /// Sanity-check hooks + run inline TOML filter tests
    Verify {
        /// Run tests only for this filter name
        #[arg(long)]
        filter: Option<String>,
        /// Fail if any filter has no inline tests (CI mode)
        #[arg(long)]
        require_all: bool,
    },

    /// ruff — check/format output that fits in chat
    Ruff {
        /// Ruff arguments (e.g., check, format --check)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// pytest — red tests first, fluff last
    Pytest {
        /// Pytest arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// mypy — type errors grouped for humans
    Mypy {
        /// Mypy arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// rake test — Minitest without the wallpaper (Ruby)
    Rake {
        /// Rake arguments (e.g., test, test TEST=path/to/test.rb)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// RuboCop — style cops, compact docket (Ruby)
    Rubocop {
        /// RuboCop arguments (e.g., --auto-correct, -A)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// RSpec — examples that failed, not the whole sonnet
    Rspec {
        /// RSpec arguments (e.g., spec/models, --tag focus)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// pip / uv — lists and installs without the spam
    Pip {
        /// Pip arguments (e.g., list, outdated, install)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// go — test/build output that respects your context window
    Go {
        #[command(subcommand)]
        command: GoCommands,
    },

    /// Graphite (gt) — stacked PRs, unstacked verbosity
    Gt {
        #[command(subcommand)]
        command: GtCommands,
    },

    /// golangci-lint — many linters, one tight transcript
    #[command(name = "golangci-lint")]
    GolangciLint {
        /// golangci-lint arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Hook rewrite audit — flip on TOK_HOOK_AUDIT=1 first
    #[command(name = "hook-audit")]
    HookAudit {
        /// Show entries from last N days (0 = all time)
        #[arg(short, long, default_value = "7")]
        since: u64,
    },

    /// “git status” → `tok git status` (hooks swear by this)
    ///
    /// Exits 0 and prints the rewritten command if supported.
    /// Exits 1 with no output if the command has no TOK equivalent.
    ///
    /// Used by Claude Code, Gemini CLI, and other LLM hooks:
    ///   REWRITTEN=$(tok rewrite "$CMD") || exit 0
    Rewrite {
        /// Raw command to rewrite (e.g. "git status", "cargo test && git push")
        /// Accepts multiple args: `tok rewrite ls -al` is equivalent to `tok rewrite "ls -al"`
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Structural code memory — index, search, and analyze your codebase
    Mem {
        #[command(subcommand)]
        command: MemCommands,
    },

    /// Code-indexing and annotation engine — headers, manifests, wiki
    Forgemap {
        #[command(subcommand)]
        command: ForgemapCommands,
    },

    /// stdin JSON hook handlers (Gemini, Copilot, …)
    Hook {
        #[command(subcommand)]
        command: HookCommands,
    },

    /// Inspect text for sensitive data (security dry-run)
    #[command(name = "security-inspect")]
    SecurityInspect {
        /// File to inspect (use - for stdin)
        file: PathBuf,
        /// Show detailed report
        #[arg(long)]
        report: bool,
    },

    /// Check SLM runtime health and configuration
    Doctor {
        /// Check SLM runtime specifically
        #[arg(long)]
        slm: bool,
    },

    /// Full command manual — every tok command with descriptions
    Man {
        /// Filter manual to a specific section (e.g. "git", "mem", "security")
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        filter: Vec<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum HookCommands {
    /// Gemini BeforeTool — JSON on stdin, magic on stdout
    Gemini,
    /// Copilot preToolUse — same deal, different vendor
    Copilot,
}

#[derive(Subcommand)]
pub(crate) enum MemCommands {
    /// Index a directory — extract symbols, relationships, and structure
    Index {
        /// Path to the directory to index
        #[arg(default_value = ".")]
        path: String,
        /// Repository identifier (defaults to directory name)
        #[arg(long)]
        repo_id: Option<String>,
        /// Git branch to associate with the index
        #[arg(long, default_value = "main")]
        branch: String,
        /// Re-index only changed files (keep existing data)
        #[arg(long)]
        incremental: bool,
        /// Wipe all existing data for this repo before indexing
        #[arg(long)]
        clear: bool,
    },

    /// Full-text search across indexed symbols (BM25 ranking)
    Search {
        /// Natural-language or keyword query
        query: String,
        /// Scope to a specific repository
        #[arg(long)]
        repo_id: Option<String>,
        /// Filter by symbol kind (Function, Class, Struct, Trait, Interface, etc.)
        #[arg(long)]
        kind: Option<String>,
        /// Max results
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },

    /// Find a symbol by exact or fuzzy name match
    Find {
        /// Symbol name (exact match by default)
        name: String,
        /// Enable fuzzy/substring matching
        #[arg(long)]
        fuzzy: bool,
        /// Scope to a specific repository
        #[arg(long)]
        repo_id: Option<String>,
        /// Filter by symbol kind
        #[arg(long)]
        kind: Option<String>,
        /// Max results
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },

    /// Show full context for a symbol (callers, callees, type refs)
    Context {
        /// Symbol name or ID
        name: String,
        /// Scope to a specific repository
        #[arg(long)]
        repo_id: Option<String>,
    },

    /// Analyze relationships by type (callers, callees, hierarchy, imports)
    Relations {
        /// Symbol name or ID
        name: String,
        /// Relationship type: find_callers, find_callees, class_hierarchy, imports, exporters, type_usages
        #[arg(long, default_value = "find_callers")]
        query_type: String,
        /// Traversal depth
        #[arg(long, default_value = "2")]
        depth: u32,
        /// Max results
        #[arg(short, long, default_value = "50")]
        limit: usize,
        /// Scope to a specific repository
        #[arg(long)]
        repo_id: Option<String>,
    },

    /// Blast radius impact analysis — who breaks if this symbol changes?
    Impact {
        /// Symbol name or ID
        name: String,
        /// Direction: upstream, downstream, both
        #[arg(long, default_value = "both")]
        direction: String,
        /// Traversal depth
        #[arg(long, default_value = "3")]
        depth: u32,
        /// Max results
        #[arg(short, long, default_value = "100")]
        limit: usize,
        /// Scope to a specific repository
        #[arg(long)]
        repo_id: Option<String>,
    },

    /// List all indexed repositories
    Repos,

    /// Show index statistics and health for a repository
    Status {
        /// Repository identifier (defaults to current directory name)
        repo_id: Option<String>,
    },

    /// Remove an indexed repository from the memory database
    Forget {
        /// Repository identifier to remove
        repo_id: String,
    },

    /// What changed in a time window — six scoring modes
    Evolution {
        /// Start of time window (ISO-8601, e.g. 2026-01-01T00:00:00Z)
        #[arg(long)]
        from: String,
        /// End of time window (ISO-8601)
        #[arg(long)]
        to: String,
        /// Scoring mode: compound, impact, novel, recent, directional, overview
        #[arg(long, default_value = "compound")]
        mode: String,
        /// Repository identifier
        #[arg(long)]
        repo_id: Option<String>,
        /// Max symbols to return
        #[arg(long, default_value = "50")]
        max_symbols: usize,
    },

    /// Full change history of a specific symbol
    Timeline {
        /// Symbol name
        name: String,
        /// Scope to a specific repository
        #[arg(long)]
        repo_id: Option<String>,
    },

    /// Session continuity — what changed since last session
    Changes {
        /// Last episode ID from previous session
        #[arg(long)]
        since_episode: Option<String>,
        /// Last reference timestamp (ISO-8601 fallback)
        #[arg(long)]
        since_time: Option<String>,
        /// Repository identifier
        #[arg(long)]
        repo_id: Option<String>,
    },

    /// Detect which symbols are affected by changed files
    Detect {
        /// Changed file paths (relative to repo root)
        #[arg(required = true, num_args = 1..)]
        files: Vec<String>,
        /// Repository identifier
        #[arg(long)]
        repo_id: Option<String>,
    },

    /// Find most central symbols (highest connectivity)
    Central {
        /// Repository identifier
        #[arg(long)]
        repo_id: Option<String>,
        /// Max results
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },

    /// Find bridge symbols connecting separate subgraphs
    Bridges {
        /// Repository identifier
        #[arg(long)]
        repo_id: Option<String>,
        /// Max results
        #[arg(short, long, default_value = "15")]
        limit: usize,
    },

    /// Detect symbol communities (connected components)
    Communities {
        /// Repository identifier
        #[arg(long)]
        repo_id: Option<String>,
        /// Minimum community size
        #[arg(long, default_value = "3")]
        min_size: usize,
        /// Max communities to show
        #[arg(short, long, default_value = "50")]
        limit: usize,
    },

    /// Find symbols with zero inbound references (potential dead code)
    #[command(name = "dead-code")]
    DeadCode {
        /// Repository identifier
        #[arg(long)]
        repo_id: Option<String>,
        /// Include test symbols in results
        #[arg(long)]
        include_tests: bool,
        /// Max results
        #[arg(short, long, default_value = "50")]
        limit: usize,
    },

    /// Estimate cyclomatic complexity for functions
    Complexity {
        /// Repository identifier
        #[arg(long)]
        repo_id: Option<String>,
        /// Max results
        #[arg(short, long, default_value = "20")]
        limit: usize,
        /// Minimum complexity to report
        #[arg(long, default_value = "5")]
        min_complexity: u32,
    },
}

#[derive(Subcommand)]
pub(crate) enum ForgemapCommands {
    /// First-time annotation pass — inject ForgeMap headers into source files
    Init {
        /// Path to scan (file or directory)
        #[arg(default_value = ".")]
        path: String,
        /// Repository root directory
        #[arg(long)]
        repo_root: Option<String>,
        /// Glob patterns to exclude
        #[arg(long, num_args = 1..)]
        exclude: Vec<String>,
        /// File extensions to include
        #[arg(long, num_args = 1..)]
        extensions: Vec<String>,
        /// Preview changes without writing
        #[arg(long)]
        dry_run: bool,
        /// Re-annotate already-annotated files
        #[arg(long)]
        force: bool,
        /// Model ID for agent: line
        #[arg(long, default_value = "forgemap-cli (no-llm)")]
        model: String,
        /// Session ID (auto-generated if omitted)
        #[arg(long)]
        session_id: Option<String>,
    },

    /// Annotate only files missing a header
    Update {
        /// Path to scan
        #[arg(default_value = ".")]
        path: String,
        /// Repository root directory
        #[arg(long)]
        repo_root: Option<String>,
        /// Glob patterns to exclude
        #[arg(long, num_args = 1..)]
        exclude: Vec<String>,
        /// File extensions to include
        #[arg(long, num_args = 1..)]
        extensions: Vec<String>,
        /// Preview changes without writing
        #[arg(long)]
        dry_run: bool,
        /// Model ID for agent: line
        #[arg(long, default_value = "forgemap-cli (no-llm)")]
        model: String,
        /// Session ID (auto-generated if omitted)
        #[arg(long)]
        session_id: Option<String>,
    },

    /// Coverage report — exit 1 if any files are unannotated
    Check {
        /// Path to scan
        #[arg(default_value = ".")]
        path: String,
        /// Repository root directory
        #[arg(long)]
        repo_root: Option<String>,
        /// Glob patterns to exclude
        #[arg(long, num_args = 1..)]
        exclude: Vec<String>,
        /// File extensions to include
        #[arg(long, num_args = 1..)]
        extensions: Vec<String>,
    },

    /// Update exports:/used_by: only — never touches rules:/agent:/message:
    Refresh {
        /// Path to scan
        #[arg(default_value = ".")]
        path: String,
        /// Repository root directory
        #[arg(long)]
        repo_root: Option<String>,
        /// Glob patterns to exclude
        #[arg(long, num_args = 1..)]
        exclude: Vec<String>,
        /// File extensions to include
        #[arg(long, num_args = 1..)]
        extensions: Vec<String>,
        /// Preview changes without writing
        #[arg(long)]
        dry_run: bool,
    },

    /// Generate .forgemap project manifest at the repo root
    Manifest {
        /// Path to scan
        #[arg(default_value = ".")]
        path: String,
        /// Repository root directory
        #[arg(long)]
        repo_root: Option<String>,
        /// Glob patterns to exclude
        #[arg(long, num_args = 1..)]
        exclude: Vec<String>,
        /// File extensions to include
        #[arg(long, num_args = 1..)]
        extensions: Vec<String>,
        /// Preview changes without writing
        #[arg(long)]
        dry_run: bool,
        /// Model ID for agent session
        #[arg(long, default_value = "forgemap-cli (no-llm)")]
        model: String,
        /// Session ID (auto-generated if omitted)
        #[arg(long)]
        session_id: Option<String>,
    },

    /// Emit per-file Obsidian vault or regenerate project wiki
    Wiki {
        #[command(subcommand)]
        command: ForgemapWikiCommands,
    },

    /// Install pre-commit hook and tool prompt files
    Install {
        /// Tool prompt files to install (claude, cursor, copilot)
        #[arg(long, num_args = 1.., default_values_t = vec!["claude".to_string(), "cursor".to_string()])]
        tools: Vec<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum ForgemapWikiCommands {
    /// Emit per-file Obsidian vault
    Bootstrap {
        /// Path to scan
        #[arg(default_value = ".")]
        path: String,
        /// Output directory for the wiki vault
        #[arg(long, default_value = "docs/wiki")]
        out: String,
        /// Repository root directory
        #[arg(long)]
        repo_root: Option<String>,
        /// Glob patterns to exclude
        #[arg(long, num_args = 1..)]
        exclude: Vec<String>,
        /// File extensions to include
        #[arg(long, num_args = 1..)]
        extensions: Vec<String>,
    },

    /// Regenerate narrative project wiki
    Sync {
        /// Path to scan
        #[arg(default_value = ".")]
        path: String,
        /// Output file path
        #[arg(long, default_value = "docs/forgemap-wiki.md")]
        out: String,
        /// Repository root directory
        #[arg(long)]
        repo_root: Option<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum GitCommands {
    /// diff — just the juicy hunks
    Diff {
        /// Git arguments (supports all git diff flags like --stat, --cached, etc)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// log — one line per commit, none of the saga
    Log {
        /// Git arguments (supports all git log flags like --oneline, --graph, --all)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// status — short, sweet, still git
    Status {
        /// Git arguments (supports all git status flags like --porcelain, --short, -s)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// show — summary + stat + diff that fits
    Show {
        /// Git arguments (supports all git show flags)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// add — usually just “ok”
    Add {
        /// Files and flags to add (supports all git add flags like -A, -p, --all, etc)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// commit — “ok” + short hash
    Commit {
        /// Git commit arguments (supports -a, -m, --amend, --allow-empty, etc)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// push — “ok” + branch
    Push {
        /// Git push arguments (supports -u, remote, branch, etc.)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// pull — “ok” + tiny stats
    Pull {
        /// Git pull arguments (supports --rebase, remote, branch, etc.)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// branch — who’s who without the parade
    Branch {
        /// Git branch arguments (supports -d, -D, -m, etc.)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// fetch — “ok” + ref count
    Fetch {
        /// Git fetch arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// stash — list/show/pop without the scroll
    Stash {
        /// Subcommand: list, show, pop, apply, drop, push
        subcommand: Option<String>,
        /// Additional arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// worktree — extra checkouts, compact roster
    Worktree {
        /// Git worktree arguments (add, remove, prune, or empty for list)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Anything else git can do — we get out of the way
    #[command(external_subcommand)]
    Other(Vec<OsString>),
}

#[derive(Subcommand)]
pub(crate) enum PnpmCommands {
    /// list — dependency tree, not dependency encyclopedia
    List {
        /// Depth level (default: 0)
        #[arg(short, long, default_value = "0")]
        depth: usize,
        /// Additional pnpm arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// outdated — “pkg: old → new”, that’s it
    Outdated {
        /// Additional pnpm arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// install — fewer progress-bar fireworks
    Install {
        /// Packages to install
        packages: Vec<String>,
        /// Additional pnpm arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// build — straight passthrough when we’re not opinionated
    Build {
        /// Additional build arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// typecheck — hands off to the tsc filter
    Typecheck {
        /// Additional typecheck arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Other pnpm spells — raw passthrough
    #[command(external_subcommand)]
    Other(Vec<OsString>),
}

#[derive(Subcommand)]
pub(crate) enum DockerCommands {
    /// ps — who’s running, briefly
    Ps,
    /// images — layers, not novels
    Images,
    /// logs — deduped tail party
    Logs { container: String },
    /// compose — stacks without the stack trace of text
    Compose {
        #[command(subcommand)]
        command: ComposeCommands,
    },
    /// Passthrough: runs any unsupported docker subcommand directly
    #[command(external_subcommand)]
    Other(Vec<OsString>),
}

#[derive(Subcommand)]
pub(crate) enum ComposeCommands {
    /// compose ps — services at a glance
    Ps,
    /// compose logs — same dedupe magic
    Logs {
        /// Optional service name
        service: Option<String>,
    },
    /// compose build — summary, not screenplay
    Build {
        /// Optional service name
        service: Option<String>,
    },
    /// compose <wildcard> — passthrough
    #[command(external_subcommand)]
    Other(Vec<OsString>),
}

#[derive(Subcommand)]
pub(crate) enum KubectlCommands {
    /// pods — who’s up, who’s pending
    Pods {
        #[arg(short, long)]
        namespace: Option<String>,
        /// All namespaces
        #[arg(short = 'A', long)]
        all: bool,
    },
    /// services — names and ports, trimmed
    Services {
        #[arg(short, long)]
        namespace: Option<String>,
        /// All namespaces
        #[arg(short = 'A', long)]
        all: bool,
    },
    /// logs — pod gossip, condensed
    Logs {
        pod: String,
        #[arg(short, long)]
        container: Option<String>,
    },
    /// kubectl <rest> — full power, zero filter
    #[command(external_subcommand)]
    Other(Vec<OsString>),
}

#[derive(Subcommand)]
pub(crate) enum VitestCommands {
    /// run — mostly failures, ~90% fewer tokens
    Run {
        /// Additional vitest arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum PrismaCommands {
    /// generate — client code, zero ASCII confetti
    Generate {
        /// Additional prisma arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// migrate — schema travel log, compact
    Migrate {
        #[command(subcommand)]
        command: PrismaMigrateCommands,
    },
    /// db push — “just make it match” energy
    DbPush {
        /// Additional prisma arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum PrismaMigrateCommands {
    /// dev — cook a migration, apply it, move on
    Dev {
        /// Migration name
        #[arg(short, long)]
        name: Option<String>,
        /// Additional arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// status — pending vs applied, no drama
    Status {
        /// Additional arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// deploy — ship migrations like an adult
    Deploy {
        /// Additional arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum CargoCommands {
    /// build — skip the “Compiling…” ticker, keep the errors
    Build {
        /// Additional cargo build arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// test — red tests first, green noise optional
    Test {
        /// Additional cargo test arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// clippy — lints grouped so you fix in batches
    Clippy {
        /// Additional cargo clippy arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// check — fast typecheck, minus the status spam
    Check {
        /// Additional cargo check arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// install — less dependency theatre, same binary
    Install {
        /// Additional cargo install arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// nextest — parallel tests, failures in focus
    Nextest {
        /// Additional cargo nextest arguments (e.g., run, list, --lib)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// cargo <your thing> — full passthrough
    #[command(external_subcommand)]
    Other(Vec<OsString>),
}

#[derive(Subcommand)]
pub(crate) enum DotnetCommands {
    /// build — MSBuild murmur, not shout
    Build {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// test — Xunit without the XML wall
    Test {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// restore — NuGet without the scroll
    Restore {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// format — dotnet-format, trimmed transcript
    Format {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// dotnet <else> — we step aside
    #[command(external_subcommand)]
    Other(Vec<OsString>),
}

#[derive(Subcommand)]
pub(crate) enum GoCommands {
    /// test — JSON stream, ~90% fewer tokens
    Test {
        /// Additional go test arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// build — errors loud, chatter low
    Build {
        /// Additional go build arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// vet — static checks, compact receipts
    Vet {
        /// Additional go vet arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// go <wildcard> — raw mode
    #[command(external_subcommand)]
    Other(Vec<OsString>),
}

/// TOK-only subcommands that should never fall back to raw execution.
/// If Clap fails to parse these, show the Clap error directly.
const TOK_META_COMMANDS: &[&str] = &[
    "gain",
    "discover",
    "learn",
    "init",
    "config",
    "proxy",
    "hook-audit",
    "cc-economics",
    "verify",
    "trust",
    "untrust",
    "session",
    "rewrite",
    "man",
];

fn run_fallback(parse_error: clap::Error) -> Result<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // No args → show branded welcome screen
    if args.is_empty() {
        print_welcome_screen();
        return Ok(0);
    }

    // TOK meta-commands should never fall back to raw execution.
    // e.g. `tok gain --badtypo` should show Clap's error, not try to run `gain` from $PATH.
    if TOK_META_COMMANDS.contains(&args[0].as_str()) {
        parse_error.exit();
    }

    let raw_command = args.join(" ");
    let error_message = core::utils::strip_ansi(&parse_error.to_string());

    // Start timer before execution to capture actual command runtime
    let timer = core::tracking::TimedExecution::start();

    // TOML filter lookup — bypass with TOK_NO_TOML=1
    // Use basename of args[0] so absolute paths (/usr/bin/make) still match "^make\b".
    let lookup_cmd = {
        let base = std::path::Path::new(&args[0])
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| args[0].clone());
        std::iter::once(base.as_str())
            .chain(args[1..].iter().map(|s| s.as_str()))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let toml_match = if std::env::var("TOK_NO_TOML").ok().as_deref() == Some("1") {
        None
    } else {
        core::toml_filter::find_matching_filter(&lookup_cmd)
    };

    if let Some(filter) = toml_match {
        // TOML match: capture stdout for filtering
        let result = core::utils::resolved_command(&args[0])
            .args(&args[1..])
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::piped()) // capture
            .stderr(std::process::Stdio::inherit()) // stderr always direct
            .output();

        match result {
            Ok(output) => {
                let exit_code = core::utils::exit_code_from_output(&output, &raw_command);
                let stdout_raw = String::from_utf8_lossy(&output.stdout);

                // Tee raw output BEFORE filtering on failure — lets LLM re-read if needed
                let tee_hint = if !output.status.success() {
                    core::tee::tee_and_hint(&stdout_raw, &raw_command, exit_code)
                } else {
                    None
                };

                let filtered = core::toml_filter::apply_filter(filter, &stdout_raw);
                println!("{}", filtered);
                if let Some(hint) = tee_hint {
                    println!("{}", hint);
                }

                timer.track(
                    &raw_command,
                    &format!("tok:toml {}", raw_command),
                    &stdout_raw,
                    &filtered,
                );
                core::tracking::record_parse_failure_silent(&raw_command, &error_message, true);

                Ok(exit_code)
            }
            Err(e) => {
                // Command not found — same behaviour as no-TOML path
                core::tracking::record_parse_failure_silent(&raw_command, &error_message, false);
                eprintln!("[tok: {}]", e);
                Ok(127)
            }
        }
    } else {
        // No TOML match: original passthrough behaviour (Stdio::inherit, streaming)
        let status = core::utils::resolved_command(&args[0])
            .args(&args[1..])
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status();

        match status {
            Ok(s) => {
                timer.track_passthrough(&raw_command, &format!("tok fallback: {}", raw_command));

                core::tracking::record_parse_failure_silent(&raw_command, &error_message, true);

                Ok(core::utils::exit_code_from_status(&s, &raw_command))
            }
            Err(e) => {
                core::tracking::record_parse_failure_silent(&raw_command, &error_message, false);
                // Command not found or other OS error — single message, no duplicate Clap error
                eprintln!("[tok: {}]", e);
                Ok(127)
            }
        }
    }
}

#[derive(Subcommand)]
pub(crate) enum GtCommands {
    /// log — stack story, short chapters
    Log {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// submit — ship the stack, skip the soliloquy
    Submit {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// sync — trunk + branches, tight summary
    Sync {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// restack — replay commits, fewer lines
    Restack {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// create — new branch/stack slice, compact
    Create {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// branch — info and moves, Graphite-style
    Branch {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// gt <anything> — passthrough when we’re not special-casing
    #[command(external_subcommand)]
    Other(Vec<OsString>),
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn print_version_banner() {
    print_welcome_screen();
}

fn print_welcome_screen() {
    if !std::io::stdout().is_terminal() {
        println!("tok {VERSION}");
        return;
    }

    let t = |s: &str| s.bright_cyan();
    let o = |s: &str| s.blue();
    let k = |s: &str| s.bright_yellow();

    // ── ASCII art banner ──────────────────────────────────────────────
    println!("{}{}{}", t("  ████████╗"), o("  ██████╗ "), k("  ██╗  ██╗"));
    println!("{}{}{}", t("  ╚══██╔══╝"), o(" ██╔═══██╗"), k("  ██║ ██╔╝"));
    println!("{}{}{}", t("     ██║   "), o(" ██║   ██║"), k("  █████╔╝ "));
    println!("{}{}{}", t("     ██║   "), o(" ██║   ██║"), k("  ██╔═██╗ "));
    println!("{}{}{}", t("     ██║   "), o("  ╚████╔╝ "), k("  ██║  ██╗"));
    println!("{}{}{}", t("     ╚═╝   "), o("  ╚═══╝  "), k("  ╚═╝  ╚═╝"));
    println!();

    // ── Version / Author ──────────────────────────────────────────────
    println!(
        "  {} {} {}",
        "T O K".bright_cyan().bold(),
        format!("v{VERSION}").bright_white().bold(),
        "— Token Optimization Kit".bright_black()
    );
    println!(
        "  {}",
        "Squeeze noisy CLI output before it hits your LLM".bright_black()
    );
    println!();
    println!(
        "  {} {}",
        "Author:".bright_black(),
        "MantisWare (Waldo Marais)".white()
    );
    println!();

    // ── Installation Status (boxed) ───────────────────────────────────
    let statuses = hooks::init::detect_agent_statuses();

    let box_width = 64;
    let border = "─".repeat(box_width - 2);
    let title = " Installation Status ";
    let title_border_len = box_width - 2 - title.len();
    let title_line = format!(
        "  {}{}{}{}",
        "┌".bright_blue(),
        title.bright_white().bold(),
        "─".repeat(title_border_len).bright_blue(),
        "┐".bright_blue()
    );
    println!("{title_line}");

    let check = "\u{2714}".green();
    let warn = "!".yellow();

    let mut installed_count = 0usize;
    let total = statuses.len();

    // Inner width = box_width - 2 (between │ and │)
    let inner = box_width - 2;

    for s in &statuses {
        if s.installed {
            installed_count += 1;
        }
        let icon = if s.installed { &check } else { &warn };
        let name_pad = format!("{:<16}", s.name);
        let detail = &s.detail;
        // visible: "  ✔ Name             detail"
        let visible_len = 2 + 1 + 1 + 16 + 2 + detail.len();
        let pad = if visible_len < inner {
            " ".repeat(inner - visible_len)
        } else {
            String::new()
        };
        println!(
            "  {}  {} {}  {}{}{}",
            "│".bright_blue(),
            icon,
            name_pad,
            detail,
            pad,
            "│".bright_blue()
        );
    }

    // empty line
    println!(
        "  {}{}{}",
        "│".bright_blue(),
        " ".repeat(inner),
        "│".bright_blue()
    );

    // summary line
    let summary_icon = if installed_count == total {
        &check
    } else {
        &warn
    };
    // visible: "  ✔ N/M agents configured"
    let summary_visible_text = format!("{}/{} agents configured", installed_count, total);
    let summary_visible_len = 2 + 1 + 1 + summary_visible_text.len();
    let summary_pad = if summary_visible_len < inner {
        " ".repeat(inner - summary_visible_len)
    } else {
        String::new()
    };
    println!(
        "  {}  {} {}{}{}",
        "│".bright_blue(),
        summary_icon,
        summary_visible_text,
        summary_pad,
        "│".bright_blue()
    );

    println!(
        "  {}{}{}",
        "└".bright_blue(),
        border.bright_blue(),
        "┘".bright_blue()
    );
    println!();

    // ── Quick Start Guide (boxed) ─────────────────────────────────────
    let guide_width = 80;
    let guide_border = "─".repeat(guide_width - 2);
    let guide_inner = guide_width - 2;

    let guide_title = " Quick Start Guide ";
    let guide_title_border_len = guide_width - 2 - guide_title.len();
    println!(
        "  {}{}{}{}",
        "┌".bright_blue(),
        guide_title.bright_white().bold(),
        "─".repeat(guide_title_border_len).bright_blue(),
        "┐".bright_blue()
    );

    let cmd_col = 36;

    let print_guide_row = |cmd: &str, desc: &str| {
        let cmd_pad = if cmd.len() < cmd_col {
            " ".repeat(cmd_col - cmd.len())
        } else {
            " ".to_string()
        };
        let content_len = 2 + cmd.len() + cmd_pad.len() + desc.len();
        let row_pad = if content_len < guide_inner {
            " ".repeat(guide_inner - content_len)
        } else {
            String::new()
        };
        println!(
            "  {}  {}{}{}{}{}",
            "│".bright_blue(),
            cmd.bright_yellow(),
            cmd_pad,
            desc.bright_black(),
            row_pad,
            "│".bright_blue()
        );
    };

    let print_guide_header = |text: &str| {
        let pad_len = guide_inner.saturating_sub(2 + text.len());
        println!(
            "  {}  {}{}{}",
            "│".bright_blue(),
            text.bright_white().bold(),
            " ".repeat(pad_len),
            "│".bright_blue()
        );
    };

    let print_guide_spacer = || {
        println!(
            "  {}{}{}",
            "│".bright_blue(),
            " ".repeat(guide_inner),
            "│".bright_blue()
        );
    };

    // ── Setup ─────────────────────────────────────────────────────────
    print_guide_header("Setup");
    print_guide_row("tok init -g", "Install for Claude Code (recommended)");
    print_guide_row("tok init -g --agent cursor", "Install for Cursor");
    print_guide_row("tok init -g --gemini", "Install for Gemini CLI");
    print_guide_row("tok init --codex", "Install for Codex CLI");
    print_guide_row("tok init -g --opencode", "Install for OpenCode");
    print_guide_row("tok init --copilot", "Install for GitHub Copilot");
    print_guide_row("tok init --all", "Install for ALL agents at once");

    print_guide_spacer();

    // ── Usage — Filters ───────────────────────────────────────────────
    print_guide_header("Usage \u{2014} Filters");
    print_guide_row("tok <command>", "Any command \u{2014} auto-filtered");
    print_guide_row("tok git status", "Git without the wall of text");
    print_guide_row("tok cargo test", "Rust tests, failures only");
    print_guide_row("tok npm run <script>", "npm with boilerplate stripped");
    print_guide_row("tok pnpm install", "pnpm on quiet-room mode");
    print_guide_row("tok docker ps", "Containers at a glance");
    print_guide_row("tok kubectl pods", "Pod status, compact");
    print_guide_row("tok go test ./...", "Go tests, ~90% fewer tokens");
    print_guide_row("tok pytest", "Red tests first, fluff last");
    print_guide_row("tok ruff check .", "Python linting, compact");
    print_guide_row("tok dotnet test", "xUnit without the XML wall");
    print_guide_row("tok vitest run", "Vitest failures loud, noise quiet");
    print_guide_row("tok playwright test", "E2E results without the novel");
    print_guide_row("tok prisma generate", "Prisma, zero ASCII confetti");
    print_guide_row("tok tsc", "TypeScript errors, grouped");
    print_guide_row("tok lint", "ESLint violations by rule/file");
    print_guide_row("tok prettier --check .", "Who needs formatting, fast");
    print_guide_row("tok rspec", "RSpec failures, not the whole sonnet");
    print_guide_row("tok mypy .", "Type errors grouped for humans");

    print_guide_spacer();

    // ── Usage — Utilities ─────────────────────────────────────────────
    print_guide_header("Usage \u{2014} Utilities");
    print_guide_row("tok ls -la", "ls, fewer columns for LLMs");
    print_guide_row("tok tree", "Tree you can scroll past");
    print_guide_row("tok find . -name '*.rs'", "Find with compact tree output");
    print_guide_row("tok grep <pattern>", "Grep, grouped and trimmed");
    print_guide_row("tok read <file>", "Smart-filtered file content");
    print_guide_row("tok smart <file>", "Two-line file summary (local)");
    print_guide_row("tok diff <a> <b>", "Only the lines that moved");
    print_guide_row("tok log <file>", "Logs deduplicated, story kept");
    print_guide_row("tok json <file>", "JSON shrunk or --schema shapes");
    print_guide_row("tok curl <url>", "curl with JSON auto-detected");
    print_guide_row("tok wget <url>", "wget sans progress bars");
    print_guide_row("tok wc <file>", "Counts without decorative padding");
    print_guide_row("tok env", "Env vars filtered, secrets hidden");
    print_guide_row("tok deps", "Dependencies, not the full novel");
    print_guide_row("tok err <command>", "Run anything, print errors only");
    print_guide_row("tok summary <command>", "Heuristic summary of output");
    print_guide_row("tok proxy <cmd>", "Raw passthrough (still tracks stats)");

    print_guide_spacer();

    // ── Usage — Analytics ─────────────────────────────────────────────
    print_guide_header("Usage \u{2014} Analytics");
    print_guide_row("tok gain", "Token savings stats");
    print_guide_row("tok gain --graph", "ASCII graph of daily savings");
    print_guide_row("tok gain --history", "Full command history");
    print_guide_row("tok cc-economics", "Claude spend vs tok savings");
    print_guide_row("tok discover", "Find missed TOK opportunities");
    print_guide_row("tok session", "Usage stats across sessions");
    print_guide_row("tok learn", "Learn CLI fixes from past mistakes");

    print_guide_spacer();

    // ── Usage — Security ──────────────────────────────────────────────
    print_guide_header("Usage \u{2014} Security");
    print_guide_row(
        "tok --security <command>",
        "Enable sensitive-data obfuscation",
    );
    print_guide_row(
        "tok security-inspect <text>",
        "Dry-run: inspect text for secrets",
    );
    print_guide_row("tok doctor --slm", "Check SLM runtime health");

    print_guide_spacer();

    // ── Usage — Code Intelligence ─────────────────────────────────────
    print_guide_header("Usage \u{2014} Code Intelligence");
    print_guide_row("tok mem index <dir>", "Index symbols and structure");
    print_guide_row("tok mem search <query>", "Full-text search (BM25)");
    print_guide_row("tok mem find <symbol>", "Find symbol by name");
    print_guide_row("tok mem context <symbol>", "Callers, callees, type refs");
    print_guide_row("tok mem impact <symbol>", "Blast radius analysis");
    print_guide_row("tok mem dead-code", "Find zero-reference symbols");
    print_guide_row("tok forgemap init", "Inject ForgeMap source headers");
    print_guide_row("tok forgemap manifest", "Generate .forgemap manifest");
    print_guide_row("tok forgemap check", "Coverage report for headers");
    print_guide_row("tok forgemap wiki bootstrap", "Emit Obsidian vault");

    print_guide_spacer();

    // ── Usage — Configuration ─────────────────────────────────────────
    print_guide_header("Usage \u{2014} Configuration");
    print_guide_row("tok config", "View or scaffold tok config");
    print_guide_row("tok trust", "Trust local .tok filter recipes");
    print_guide_row("tok untrust", "Remove trusted filter recipes");
    print_guide_row("tok verify", "Sanity-check hooks and filters");
    print_guide_row("tok man", "Full command manual (every command)");
    print_guide_row("tok man <topic>", "Filter manual (e.g. tok man git)");
    print_guide_row("tok --help", "All commands and flags");

    print_guide_spacer();

    // ── Documentation link ────────────────────────────────────────────
    let doc_text = "Documentation: ";
    let doc_url = "https://github.com/MantisWare/tok";
    let doc_visible = 2 + doc_text.len() + doc_url.len();
    let doc_pad = if doc_visible < guide_inner {
        " ".repeat(guide_inner - doc_visible)
    } else {
        String::new()
    };
    println!(
        "  {}  {}{}{}{}",
        "│".bright_blue(),
        doc_text.bright_black(),
        doc_url.bright_cyan(),
        doc_pad,
        "│".bright_blue()
    );

    println!(
        "  {}{}{}",
        "└".bright_blue(),
        guide_border.bright_blue(),
        "┘".bright_blue()
    );
}

// ── tok man ─────────────────────────────────────────────────────────────

struct ManSection {
    heading: &'static str,
    entries: &'static [(&'static str, &'static str)],
}

const MANUAL: &[ManSection] = &[
    ManSection {
        heading: "Git & GitHub",
        entries: &[
            ("tok git status", "Compact status output"),
            (
                "tok git log",
                "One-line-per-commit log (all git flags work)",
            ),
            ("tok git diff", "Compact diff — just the juicy hunks"),
            ("tok git show", "Summary + stat + diff that fits"),
            ("tok git add", "Ultra-compact confirmation"),
            ("tok git commit", "Ultra-compact confirmation + short hash"),
            ("tok git push", "Ultra-compact confirmation + branch"),
            ("tok git pull", "Ultra-compact confirmation + tiny stats"),
            ("tok git branch", "Branch list without the parade"),
            ("tok git fetch", "Compact fetch + ref count"),
            ("tok git stash", "List/show/pop without the scroll"),
            ("tok git worktree", "Extra checkouts, compact roster"),
            ("tok git <other>", "Any git subcommand — passthrough"),
            ("tok gh pr view <n>", "Compact PR view"),
            ("tok gh pr checks", "Compact PR checks"),
            ("tok gh run list", "Compact workflow runs"),
            ("tok gh issue list", "Compact issue list"),
            ("tok gh api", "Compact API responses"),
        ],
    },
    ManSection {
        heading: "Build & Compile",
        entries: &[
            ("tok cargo build", "Skip the Compiling ticker, keep errors"),
            ("tok cargo check", "Fast typecheck, minus status spam"),
            ("tok cargo clippy", "Clippy lints grouped by file"),
            ("tok cargo install", "Less dependency theatre"),
            ("tok cargo nextest", "Parallel tests, failures in focus"),
            ("tok tsc", "TypeScript errors grouped by file/code"),
            ("tok lint", "ESLint/Biome violations grouped by rule/file"),
            ("tok prettier --check .", "Files needing format only"),
            ("tok next build", "Next.js build with route metrics"),
            ("tok dotnet build", "MSBuild murmur, not shout"),
            ("tok dotnet restore", "NuGet without the scroll"),
            ("tok dotnet format", "dotnet-format, trimmed transcript"),
            ("tok go build", "Errors loud, chatter low"),
            ("tok go vet", "Static checks, compact receipts"),
        ],
    },
    ManSection {
        heading: "Test",
        entries: &[
            ("tok cargo test", "Failures only (~90% savings)"),
            ("tok vitest run", "Vitest failures only (~99% savings)"),
            ("tok playwright test", "E2E failures only (~94% savings)"),
            ("tok pytest", "Red tests first, fluff last"),
            (
                "tok go test ./...",
                "Go test JSON stream, ~90% fewer tokens",
            ),
            ("tok rspec", "RSpec failures, not the whole sonnet"),
            ("tok rake test", "Minitest without the wallpaper"),
            ("tok dotnet test", "Xunit without the XML wall"),
            ("tok test <cmd>", "Generic test wrapper — failures only"),
        ],
    },
    ManSection {
        heading: "JavaScript & TypeScript",
        entries: &[
            ("tok pnpm install", "Fewer progress-bar fireworks"),
            ("tok pnpm list", "Dependency tree, not encyclopedia"),
            ("tok pnpm outdated", "pkg: old → new, that's it"),
            ("tok pnpm typecheck", "Hands off to the tsc filter"),
            ("tok npm run <script>", "Boilerplate stripped, signal kept"),
            (
                "tok npx <cmd>",
                "Smart routing to tsc/eslint/prisma filters",
            ),
            ("tok prisma generate", "Client code, zero ASCII confetti"),
            ("tok prisma migrate dev", "Schema travel log, compact"),
            ("tok prisma db push", "\"Just make it match\" energy"),
        ],
    },
    ManSection {
        heading: "Python",
        entries: &[
            ("tok ruff check .", "Python linting, compact output"),
            ("tok mypy .", "Type errors grouped for humans"),
            ("tok pytest", "Red tests first, fluff last"),
            ("tok pip install <pkg>", "pip/uv without the spam"),
            ("tok pip list", "Compact package list"),
        ],
    },
    ManSection {
        heading: "Ruby",
        entries: &[
            ("tok rake test", "Minitest without the wallpaper"),
            ("tok rubocop", "RuboCop — compact docket"),
            ("tok rspec", "RSpec — failures, not the whole sonnet"),
        ],
    },
    ManSection {
        heading: "Go",
        entries: &[
            ("tok go test ./...", "JSON stream, ~90% fewer tokens"),
            ("tok go build", "Errors loud, chatter low"),
            ("tok go vet", "Static checks, compact receipts"),
            (
                "tok golangci-lint run",
                "Many linters, one tight transcript",
            ),
        ],
    },
    ManSection {
        heading: ".NET",
        entries: &[
            ("tok dotnet build", "MSBuild murmur, not shout"),
            ("tok dotnet test", "Xunit without the XML wall"),
            ("tok dotnet restore", "NuGet without the scroll"),
            ("tok dotnet format", "dotnet-format, trimmed transcript"),
        ],
    },
    ManSection {
        heading: "Graphite (Stacked PRs)",
        entries: &[
            ("tok gt log", "Stack story, short chapters"),
            ("tok gt submit", "Ship the stack, skip the soliloquy"),
            ("tok gt sync", "Trunk + branches, tight summary"),
            ("tok gt restack", "Replay commits, fewer lines"),
            ("tok gt create", "New branch/stack slice, compact"),
            ("tok gt branch", "Info and moves, Graphite-style"),
        ],
    },
    ManSection {
        heading: "Files, Search & Utilities",
        entries: &[
            ("tok ls <path>", "ls in tree format, compact"),
            ("tok tree", "tree(1) you can actually scroll past"),
            ("tok read <file>", "Smart-filtered file content"),
            (
                "tok smart <file>",
                "Two-line file summary (local, no cloud)",
            ),
            (
                "tok find . -name '*.rs'",
                "find with compact tree-ish output",
            ),
            ("tok grep <pattern>", "grep/rg grouped by file, trimmed"),
            ("tok wc <file>", "Counts without decorative padding"),
            ("tok diff <a> <b>", "Only the lines that actually moved"),
            ("tok json <file>", "Shrink values or --schema for shapes"),
            ("tok env", "Env vars filtered, secrets stay shy"),
            ("tok deps", "Dependencies without the manifest novel"),
            ("tok log <file>", "Dedupe repeats, keep the story"),
            (
                "tok err <cmd>",
                "Run anything — print errors & warnings only",
            ),
            ("tok summary <cmd>", "Heuristic summary of command output"),
            ("tok format", "Auto-picks prettier / black / ruff format"),
        ],
    },
    ManSection {
        heading: "Infrastructure",
        entries: &[
            ("tok docker ps", "Who's running, briefly"),
            ("tok docker images", "Layers, not novels"),
            ("tok docker logs <c>", "Deduplicated tail party"),
            ("tok docker compose ps", "Compose services at a glance"),
            ("tok docker compose logs", "Compose logs, same dedupe magic"),
            ("tok docker compose build", "Summary, not screenplay"),
            ("tok kubectl pods", "Who's up, who's pending"),
            ("tok kubectl services", "Names and ports, trimmed"),
            ("tok kubectl logs", "Pod gossip, condensed"),
        ],
    },
    ManSection {
        heading: "Cloud & Database",
        entries: &[
            ("tok aws <cmd>", "AWS CLI — JSON in, human-sized lines out"),
            ("tok psql <cmd>", "psql — tidy tables, fewer borders"),
        ],
    },
    ManSection {
        heading: "Network",
        entries: &[
            ("tok curl <url>", "JSON auto-detected; --schema for shapes"),
            ("tok wget <url>", "Skip the progress-bar light show"),
        ],
    },
    ManSection {
        heading: "Code Intelligence — tok mem",
        entries: &[
            (
                "tok mem index <dir>",
                "Index symbols, relationships, and structure",
            ),
            (
                "tok mem search <query>",
                "Full-text search across symbols (BM25)",
            ),
            ("tok mem find <symbol>", "Exact or fuzzy symbol lookup"),
            ("tok mem context <symbol>", "Callers, callees, type refs"),
            (
                "tok mem relations <sym>",
                "Analyze by type: callers, callees, hierarchy, imports",
            ),
            (
                "tok mem impact <symbol>",
                "Blast radius — who breaks if this changes?",
            ),
            ("tok mem dead-code", "Symbols with zero inbound references"),
            (
                "tok mem central",
                "Most central symbols (highest connectivity)",
            ),
            ("tok mem bridges", "Bridge symbols connecting subgraphs"),
            (
                "tok mem communities",
                "Detect symbol communities (connected components)",
            ),
            ("tok mem complexity <fn>", "Estimate cyclomatic complexity"),
            ("tok mem evolution", "What changed in a time window"),
            ("tok mem timeline <sym>", "Full change history of a symbol"),
            ("tok mem changes", "What changed since last session"),
            ("tok mem detect", "Symbols affected by changed files"),
            ("tok mem repos", "List all indexed repositories"),
            ("tok mem status", "Index statistics and health"),
            (
                "tok mem forget <repo>",
                "Remove indexed repo from memory DB",
            ),
        ],
    },
    ManSection {
        heading: "Code Intelligence — tok forgemap",
        entries: &[
            (
                "tok forgemap init",
                "First-time annotation — inject headers into source",
            ),
            (
                "tok forgemap update",
                "Annotate only files missing a header",
            ),
            (
                "tok forgemap check",
                "Coverage report — exit 1 if unannotated files",
            ),
            ("tok forgemap refresh", "Update exports:/used_by: only"),
            (
                "tok forgemap manifest",
                "Generate .forgemap project manifest",
            ),
            (
                "tok forgemap wiki bootstrap",
                "Emit per-file Obsidian vault",
            ),
            (
                "tok forgemap wiki sync",
                "Regenerate narrative project wiki",
            ),
            (
                "tok forgemap install",
                "Install pre-commit hook and tool prompts",
            ),
        ],
    },
    ManSection {
        heading: "Security",
        entries: &[
            (
                "tok --security <cmd>",
                "Enable security layer — obfuscate sensitive data",
            ),
            (
                "tok --no-security",
                "Disable security layer (overrides config)",
            ),
            (
                "tok --security-mode <m>",
                "Mode: observe, balanced, strict, developer",
            ),
            (
                "tok --slm",
                "Enable local SLM for semantic security scanning",
            ),
            ("tok --no-slm", "Disable local SLM (overrides config)"),
            (
                "tok security-inspect <f>",
                "Inspect text/file for sensitive data (dry-run)",
            ),
            (
                "tok doctor --slm",
                "Check SLM runtime health and configuration",
            ),
        ],
    },
    ManSection {
        heading: "Analytics & Insights",
        entries: &[
            ("tok gain", "Token savings dashboard"),
            ("tok gain --graph", "ASCII graph of daily savings"),
            ("tok gain --history", "Per-command savings history"),
            (
                "tok cc-economics",
                "Claude spend vs tok savings — receipts included",
            ),
            (
                "tok discover",
                "Mine session history for missed TOK opportunities",
            ),
            ("tok session", "Usage stats across sessions"),
            ("tok learn", "Learn CLI fixes from past mistakes"),
        ],
    },
    ManSection {
        heading: "Configuration & Setup",
        entries: &[
            ("tok init -g", "Install hooks for Claude Code (recommended)"),
            ("tok init -g --agent cursor", "Install hooks for Cursor"),
            ("tok init -g --gemini", "Install hooks for Gemini CLI"),
            ("tok init --codex", "Install hooks for Codex CLI"),
            ("tok init -g --opencode", "Install hooks for OpenCode"),
            ("tok init --copilot", "Install hooks for GitHub Copilot"),
            ("tok init --all", "Install hooks for ALL agents at once"),
            ("tok config", "View or scaffold tok config"),
            ("tok verify", "Sanity-check hooks + run TOML filter tests"),
            ("tok trust", "Trust this project's .tok filter recipes"),
            ("tok untrust", "Remove trusted local TOML filters"),
            (
                "tok proxy <cmd>",
                "Raw passthrough — still counts toward stats",
            ),
        ],
    },
    ManSection {
        heading: "Hook Internals (used by agent integrations)",
        entries: &[
            (
                "tok rewrite <cmd>",
                "Rewrite a raw command to its tok equivalent",
            ),
            ("tok hook gemini", "Gemini BeforeTool — JSON stdin handler"),
            (
                "tok hook copilot",
                "Copilot preToolUse — JSON stdin handler",
            ),
            (
                "tok hook-audit",
                "Hook rewrite audit (set TOK_HOOK_AUDIT=1 first)",
            ),
        ],
    },
    ManSection {
        heading: "Global Flags",
        entries: &[
            ("-v / -vv / -vvv", "Increase verbosity"),
            (
                "-u / --ultra-compact",
                "Maximum compression (ASCII icons, inline fields)",
            ),
            (
                "--skip-env",
                "Pass SKIP_ENV_VALIDATION=1 to child processes",
            ),
            ("--security", "Enable security/privacy layer"),
            (
                "--security-mode <mode>",
                "Security mode: observe, balanced, strict, developer",
            ),
            ("--slm / --no-slm", "Enable/disable local SLM scanning"),
        ],
    },
];

/// Print the full tok manual. Optionally filter to sections matching a query.
pub(crate) fn print_manual(filter: &[String]) {
    use colored::Colorize;

    let query = filter.join(" ").to_lowercase();

    let sections: Vec<&ManSection> = if query.is_empty() {
        MANUAL.iter().collect()
    } else {
        MANUAL
            .iter()
            .filter(|s| {
                s.heading.to_lowercase().contains(&query)
                    || s.entries.iter().any(|(cmd, desc)| {
                        cmd.to_lowercase().contains(&query) || desc.to_lowercase().contains(&query)
                    })
            })
            .collect()
    };

    if sections.is_empty() {
        println!(
            "No manual sections match {:?}. Run {} to see all.",
            query,
            "tok man".bright_yellow()
        );
        return;
    }

    println!();
    println!(
        "  {} {} {}",
        "TOK".bright_cyan().bold(),
        format!("v{VERSION}").bright_white().bold(),
        "— Command Manual".bright_black()
    );
    if !query.is_empty() {
        println!("  {} {:?}", "Filtered to:".bright_black(), query);
    }
    println!();

    let cmd_col: usize = 32;

    for section in &sections {
        println!("  {}", section.heading.bright_white().bold().underline());
        println!();
        for (cmd, desc) in section.entries {
            let pad = if cmd.len() < cmd_col {
                " ".repeat(cmd_col - cmd.len())
            } else {
                "  ".to_string()
            };
            println!("    {}{}{}", cmd.bright_yellow(), pad, desc.bright_black());
        }
        println!();
    }

    println!(
        "  {} {}",
        "Docs:".bright_black(),
        "https://github.com/MantisWare/tok".bright_cyan()
    );
    println!();
}

fn main() {
    let code = match run_cli() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("tok: {:#}", e);
            1
        }
    };
    std::process::exit(code);
}

fn run_cli() -> Result<i32> {
    // Fire-and-forget telemetry ping (1/day, non-blocking)
    core::telemetry::maybe_ping();

    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            if e.kind() == ErrorKind::DisplayHelp {
                e.exit();
            }
            if e.kind() == ErrorKind::DisplayVersion {
                print_version_banner();
                return Ok(0);
            }
            return run_fallback(e);
        }
    };

    // Warn if installed hook is outdated/missing (1/day, non-blocking).
    // Skip for Gain — it shows its own inline hook warning.
    if !matches!(cli.command, Commands::Gain { .. }) {
        hooks::hook_check::maybe_warn();
    }

    // Runtime integrity check for operational commands.
    // Meta commands (init, gain, verify, config, etc.) skip the check
    // because they don't go through the hook pipeline.
    if is_operational_command(&cli.command) {
        hooks::integrity::runtime_check()?;
    }

    cli_dispatch::dispatch(cli)
}

/// Returns true for commands that are invoked via the hook pipeline
/// (i.e., commands that process rewritten shell commands).
/// Meta commands (init, gain, verify, etc.) are excluded because
/// they are run directly by the user, not through the hook.
/// Returns true for commands that go through the hook pipeline
/// and therefore require integrity verification.
///
/// SECURITY: whitelist pattern — new commands are NOT integrity-checked
/// until explicitly added here. A forgotten command fails open (no check)
/// rather than creating false confidence about what's protected.
fn is_operational_command(cmd: &Commands) -> bool {
    matches!(
        cmd,
        Commands::Ls { .. }
            | Commands::Tree { .. }
            | Commands::Read { .. }
            | Commands::Smart { .. }
            | Commands::Git { .. }
            | Commands::Gh { .. }
            | Commands::Pnpm { .. }
            | Commands::Err { .. }
            | Commands::Test { .. }
            | Commands::Json { .. }
            | Commands::Deps { .. }
            | Commands::Env { .. }
            | Commands::Find { .. }
            | Commands::Diff { .. }
            | Commands::Log { .. }
            | Commands::Dotnet { .. }
            | Commands::Docker { .. }
            | Commands::Kubectl { .. }
            | Commands::Summary { .. }
            | Commands::Grep { .. }
            | Commands::Wget { .. }
            | Commands::Vitest { .. }
            | Commands::Prisma { .. }
            | Commands::Tsc { .. }
            | Commands::Next { .. }
            | Commands::Lint { .. }
            | Commands::Prettier { .. }
            | Commands::Playwright { .. }
            | Commands::Cargo { .. }
            | Commands::Npm { .. }
            | Commands::Npx { .. }
            | Commands::Curl { .. }
            | Commands::Ruff { .. }
            | Commands::Pytest { .. }
            | Commands::Rake { .. }
            | Commands::Rubocop { .. }
            | Commands::Rspec { .. }
            | Commands::Pip { .. }
            | Commands::Go { .. }
            | Commands::GolangciLint { .. }
            | Commands::Gt { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_git_commit_single_message() {
        let cli = Cli::try_parse_from(["tok", "git", "commit", "-m", "fix: typo"]).unwrap();
        match cli.command {
            Commands::Git {
                command: GitCommands::Commit { args },
                ..
            } => {
                assert_eq!(args, vec!["-m", "fix: typo"]);
            }
            _ => panic!("Expected Git Commit command"),
        }
    }

    #[test]
    fn test_git_commit_multiple_messages() {
        let cli = Cli::try_parse_from([
            "tok",
            "git",
            "commit",
            "-m",
            "feat: add support",
            "-m",
            "Body paragraph here.",
        ])
        .unwrap();
        match cli.command {
            Commands::Git {
                command: GitCommands::Commit { args },
                ..
            } => {
                assert_eq!(
                    args,
                    vec!["-m", "feat: add support", "-m", "Body paragraph here."]
                );
            }
            _ => panic!("Expected Git Commit command"),
        }
    }

    // #327: git commit -am "msg" was rejected by Clap
    #[test]
    fn test_git_commit_am_flag() {
        let cli = Cli::try_parse_from(["tok", "git", "commit", "-am", "quick fix"]).unwrap();
        match cli.command {
            Commands::Git {
                command: GitCommands::Commit { args },
                ..
            } => {
                assert_eq!(args, vec!["-am", "quick fix"]);
            }
            _ => panic!("Expected Git Commit command"),
        }
    }

    #[test]
    fn test_git_commit_amend() {
        let cli =
            Cli::try_parse_from(["tok", "git", "commit", "--amend", "-m", "new msg"]).unwrap();
        match cli.command {
            Commands::Git {
                command: GitCommands::Commit { args },
                ..
            } => {
                assert_eq!(args, vec!["--amend", "-m", "new msg"]);
            }
            _ => panic!("Expected Git Commit command"),
        }
    }

    #[test]
    fn test_git_global_options_parsing() {
        let cli =
            Cli::try_parse_from(["tok", "git", "--no-pager", "--no-optional-locks", "status"])
                .unwrap();
        match cli.command {
            Commands::Git {
                no_pager,
                no_optional_locks,
                bare,
                literal_pathspecs,
                ..
            } => {
                assert!(no_pager);
                assert!(no_optional_locks);
                assert!(!bare);
                assert!(!literal_pathspecs);
            }
            _ => panic!("Expected Git command"),
        }
    }

    #[test]
    fn test_git_commit_long_flag_multiple() {
        let cli = Cli::try_parse_from([
            "tok",
            "git",
            "commit",
            "--message",
            "title",
            "--message",
            "body",
            "--message",
            "footer",
        ])
        .unwrap();
        match cli.command {
            Commands::Git {
                command: GitCommands::Commit { args },
                ..
            } => {
                assert_eq!(
                    args,
                    vec![
                        "--message",
                        "title",
                        "--message",
                        "body",
                        "--message",
                        "footer"
                    ]
                );
            }
            _ => panic!("Expected Git Commit command"),
        }
    }

    #[test]
    fn test_try_parse_valid_git_status() {
        let result = Cli::try_parse_from(["tok", "git", "status"]);
        assert!(result.is_ok(), "git status should parse successfully");
    }

    #[test]
    fn test_try_parse_help_is_display_help() {
        match Cli::try_parse_from(["tok", "--help"]) {
            Err(e) => assert_eq!(e.kind(), ErrorKind::DisplayHelp),
            Ok(_) => panic!("Expected DisplayHelp error"),
        }
    }

    #[test]
    fn test_try_parse_version_is_display_version() {
        match Cli::try_parse_from(["tok", "--version"]) {
            Err(e) => assert_eq!(e.kind(), ErrorKind::DisplayVersion),
            Ok(_) => panic!("Expected DisplayVersion error"),
        }
    }

    #[test]
    fn test_print_version_banner_does_not_panic() {
        print_version_banner();
    }

    #[test]
    fn test_try_parse_unknown_subcommand_is_error() {
        match Cli::try_parse_from(["tok", "nonexistent-command"]) {
            Err(e) => assert!(!matches!(
                e.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            )),
            Ok(_) => panic!("Expected parse error for unknown subcommand"),
        }
    }

    #[test]
    fn test_try_parse_git_with_dash_c_succeeds() {
        let result = Cli::try_parse_from(["tok", "git", "-C", "/path", "status"]);
        assert!(
            result.is_ok(),
            "git -C /path status should parse successfully"
        );
        if let Ok(cli) = result {
            match cli.command {
                Commands::Git { directory, .. } => {
                    assert_eq!(directory, vec!["/path"]);
                }
                _ => panic!("Expected Git command"),
            }
        }
    }

    #[test]
    fn test_gain_failures_flag_parses() {
        let result = Cli::try_parse_from(["tok", "gain", "--failures"]);
        assert!(result.is_ok());
        if let Ok(cli) = result {
            match cli.command {
                Commands::Gain { failures, .. } => assert!(failures),
                _ => panic!("Expected Gain command"),
            }
        }
    }

    #[test]
    fn test_gain_failures_short_flag_parses() {
        let result = Cli::try_parse_from(["tok", "gain", "-F"]);
        assert!(result.is_ok());
        if let Ok(cli) = result {
            match cli.command {
                Commands::Gain { failures, .. } => assert!(failures),
                _ => panic!("Expected Gain command"),
            }
        }
    }

    #[test]
    fn test_gain_reset_flag_parses() {
        let result = Cli::try_parse_from(["tok", "gain", "--reset"]);
        assert!(result.is_ok());
        if let Ok(cli) = result {
            match cli.command {
                Commands::Gain { reset, .. } => assert!(reset),
                _ => panic!("Expected Gain command"),
            }
        }
    }

    #[test]
    fn test_meta_commands_reject_bad_flags() {
        // TOK meta-commands should produce parse errors (not fall through to raw execution).
        // Skip "proxy" because it uses trailing_var_arg (accepts any args by design).
        for cmd in TOK_META_COMMANDS {
            if matches!(*cmd, "proxy" | "rewrite" | "session" | "man") {
                continue; // these use trailing_var_arg (accept any args by design)
            }
            let result = Cli::try_parse_from(["tok", cmd, "--nonexistent-flag-xyz"]);
            assert!(
                result.is_err(),
                "Meta-command '{}' with bad flag should fail to parse",
                cmd
            );
        }
    }

    #[test]
    fn test_meta_command_list_is_complete() {
        // Verify all meta-commands are in the guard list by checking they parse with valid syntax
        let meta_cmds_that_parse = [
            vec!["tok", "gain"],
            vec!["tok", "discover"],
            vec!["tok", "learn"],
            vec!["tok", "init"],
            vec!["tok", "config"],
            vec!["tok", "proxy", "echo", "hi"],
            vec!["tok", "hook-audit"],
            vec!["tok", "cc-economics"],
            vec!["tok", "man"],
        ];
        for args in &meta_cmds_that_parse {
            let result = Cli::try_parse_from(args.iter());
            assert!(
                result.is_ok(),
                "Meta-command {:?} should parse successfully",
                args
            );
        }
    }

    #[test]
    fn test_shell_split_simple() {
        assert_eq!(
            discover::lexer::shell_split("head -50 file.php"),
            vec!["head", "-50", "file.php"]
        );
    }

    #[test]
    fn test_shell_split_double_quotes() {
        assert_eq!(
            discover::lexer::shell_split(r#"git log --format="%H %s""#),
            vec!["git", "log", "--format=%H %s"]
        );
    }

    #[test]
    fn test_shell_split_single_quotes() {
        assert_eq!(
            discover::lexer::shell_split("grep -r 'hello world' ."),
            vec!["grep", "-r", "hello world", "."]
        );
    }

    #[test]
    fn test_shell_split_single_word() {
        assert_eq!(discover::lexer::shell_split("ls"), vec!["ls"]);
    }

    #[test]
    fn test_shell_split_empty() {
        let result: Vec<String> = discover::lexer::shell_split("");
        assert!(result.is_empty());
    }

    #[test]
    fn test_rewrite_clap_multi_args() {
        // This is the bug KuSh reported: `tok rewrite ls -al` failed because
        // Clap rejected `-al` as an unknown flag. With trailing_var_arg + allow_hyphen_values,
        // multiple args are accepted and joined into a single command string.
        let cases = vec![
            vec!["tok", "rewrite", "ls", "-al"],
            vec!["tok", "rewrite", "git", "status"],
            vec!["tok", "rewrite", "npm", "exec"],
            vec!["tok", "rewrite", "cargo", "test"],
            vec!["tok", "rewrite", "du", "-sh", "."],
            vec!["tok", "rewrite", "head", "-50", "file.txt"],
        ];
        for args in &cases {
            let result = Cli::try_parse_from(args.iter());
            assert!(
                result.is_ok(),
                "tok rewrite {:?} should parse (was failing before trailing_var_arg fix)",
                &args[2..]
            );
            if let Ok(cli) = result {
                match cli.command {
                    Commands::Rewrite { ref args } => {
                        assert!(args.len() >= 2, "rewrite args should capture all tokens");
                    }
                    _ => panic!("expected Rewrite command"),
                }
            }
        }
    }

    #[test]
    fn test_rewrite_clap_quoted_single_arg() {
        // Quoted form: `tok rewrite "git status"` — single arg containing spaces
        let result = Cli::try_parse_from(["tok", "rewrite", "git status"]);
        assert!(result.is_ok());
        if let Ok(cli) = result {
            match cli.command {
                Commands::Rewrite { ref args } => {
                    assert_eq!(args.len(), 1);
                    assert_eq!(args[0], "git status");
                }
                _ => panic!("expected Rewrite command"),
            }
        }
    }
}
