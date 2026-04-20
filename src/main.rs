mod analytics;
mod cli_dispatch;
mod cmds;
mod core;
mod discover;
mod hooks;
mod learn;
mod parser;

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

    /// stdin JSON hook handlers (Gemini, Copilot, …)
    Hook {
        #[command(subcommand)]
        command: HookCommands,
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
];

fn run_fallback(parse_error: clap::Error) -> Result<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // No args → show Clap's error (user ran just "tok" with bad syntax)
    if args.is_empty() {
        parse_error.exit();
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
    if !std::io::stdout().is_terminal() {
        println!("tok {VERSION}");
        return;
    }

    let c = |s: &str| s.bright_cyan(); // T, K letters
    let b = |s: &str| s.blue(); // O left arc
    let y = |s: &str| s.bright_yellow(); // O right arc / amber accent
    let yd = |s: &str| s.yellow(); // darker yellow (lower rows)
    let g = |s: &str| s.green(); // bar accent (upper)
    let gb = |s: &str| s.bright_green(); // bar accent (lower)

    println!(
        "{}{}{}{}{}",
        c("  ████████╗"),
        b("  ██████╗ "),
        g("█"),
        y("█"),
        c("  ██╗  ██╗")
    );
    println!(
        "{}{}{}{}{}",
        c("  ╚══██╔══╝"),
        b(" ██╔═══██╗"),
        g("█"),
        y("█"),
        c("  ██║ ██╔╝")
    );
    println!(
        "{}{}{}{}{}",
        c("     ██║   "),
        b(" ██║   ██║"),
        g("█"),
        y("█"),
        c("  █████╔╝ ")
    );
    println!(
        "{}{}{}{}{}",
        c("     ██║   "),
        b(" ██║   ██║"),
        g("█"),
        yd("█"),
        c("  ██╔═██╗ ")
    );
    println!(
        "{}{}{}{}{}",
        c("     ██║   "),
        b("  ╚████╔╝"),
        gb("█"),
        yd("█"),
        c("  ██║  ██╗")
    );
    println!(
        "{}{}{}{}{}",
        c("     ╚═╝   "),
        b("  ╚═══╝ "),
        gb("█"),
        yd("█"),
        c("  ╚═╝  ╚═╝")
    );
    println!("                     {}{}", gb("▀"), yd("▀"));
    println!();
    println!(
        "  {} {} {}",
        "tok".bright_cyan().bold(),
        VERSION.bright_white().bold(),
        "— Token Optimization Kit".bright_black()
    );
    println!(
        "  {}",
        "Squeeze noisy CLI output before it hits your LLM".bright_black()
    );
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
            if matches!(*cmd, "proxy" | "rewrite" | "session") {
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
