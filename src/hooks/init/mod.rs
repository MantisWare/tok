//! Sets up TOK hooks so AI coding agents automatically route commands through TOK.

use anyhow::{Context, Result};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

use super::constants::{
    AFTER_AGENT_KEY, BEFORE_AGENT_KEY, BEFORE_TOOL_KEY, GEMINI_HOOK_FILE, HOOKS_JSON, HOOKS_SUBDIR,
    MEMORY_CACHE_PROMPT_HOOK_FILE, MEMORY_EXTRACT_HOOK_FILE, MEMORY_PROMPT_HOOK_FILE,
    MEMORY_SESSION_HOOK_FILE, POST_TOOL_USE_KEY, PRE_TOOL_USE_KEY, REWRITE_HOOK_FILE,
    SESSION_START_KEY, SETTINGS_JSON, STOP_KEY, USER_PROMPT_SUBMIT_KEY,
};
use super::integrity;

mod paths;

use paths::{
    resolve_claude_dir, resolve_codex_dir, resolve_cursor_dir, resolve_gemini_dir,
    user_opencode_plugin_path,
};

// Embedded hook script (guards before set -euo pipefail)
const REWRITE_HOOK: &str = include_str!("../../../hooks/claude/tok-rewrite.sh");

// Embedded Cursor hook script (preToolUse format)
const CURSOR_REWRITE_HOOK: &str = include_str!("../../../hooks/cursor/tok-rewrite.sh");

// Embedded Cursor sessionStart hook (injects tok awareness into conversation context)
const CURSOR_SESSION_HOOK: &str = include_str!("../../../hooks/cursor/tok-session.sh");
const SESSION_HOOK_FILE: &str = "tok-session.sh";

const MEMORY_SESSION_HOOK: &str = include_str!("../../../hooks/shared/tok-memory-session.sh");
const MEMORY_PROMPT_HOOK: &str = include_str!("../../../hooks/shared/tok-memory-prompt.sh");
const MEMORY_EXTRACT_HOOK: &str = include_str!("../../../hooks/shared/tok-memory-extract.sh");
const MEMORY_CACHE_PROMPT_HOOK: &str =
    include_str!("../../../hooks/shared/tok-memory-cache-prompt.sh");
const MEMORY_HOOK_MARKER: &str = "tok-memory-";

const GRAPH_SESSION_HOOK: &str = include_str!("../../../hooks/shared/tok-graph-session.sh");
const GRAPH_POSTEDIT_HOOK: &str = include_str!("../../../hooks/shared/tok-graph-postedit.sh");
const GRAPH_SESSION_HOOK_FILE: &str = "tok-graph-session.sh";
const GRAPH_POSTEDIT_HOOK_FILE: &str = "tok-graph-postedit.sh";
const GRAPH_HOOK_MARKER: &str = "tok-graph-";

/// Claude tools that change a file, and so invalidate part of the graph.
const EDIT_TOOL_MATCHER: &str = "Edit|Write|MultiEdit|NotebookEdit";

// Embedded OpenCode plugin (auto-rewrite)
const OPENCODE_PLUGIN: &str = include_str!("../../../hooks/opencode/tok.ts");

// Embedded slim TOK awareness instructions
const TOK_SLIM: &str = include_str!("../../../hooks/claude/tok-awareness.md");
const TOK_SLIM_CODEX: &str = include_str!("../../../hooks/codex/tok-awareness.md");

/// Template written by `tok init` when no filters.toml exists yet.
const FILTERS_TEMPLATE: &str = r#"# Project-local TOK filters — commit this file with your repo.
# Filters here override user-global and built-in filters.
# Docs: https://github.com/MantisWare/tok#custom-filters
schema_version = 1

# Example: suppress build noise from a custom tool
# [filters.my-tool]
# description = "Compact my-tool output"
# match_command = "^my-tool\\s+build"
# strip_ansi = true
# strip_lines_matching = ["^\\s*$", "^Downloading", "^Installing"]
# max_lines = 30
# on_empty = "my-tool: ok"
"#;

/// Template for user-global filters (~/.config/tok/filters.toml).
const FILTERS_GLOBAL_TEMPLATE: &str = r#"# User-global TOK filters — apply to all your projects.
# Project-local .tok/filters.toml takes precedence over these.
# Docs: https://github.com/MantisWare/tok#custom-filters
schema_version = 1

# Example: suppress noise from a tool you use everywhere
# [filters.my-global-tool]
# description = "Compact my-global-tool output"
# match_command = "^my-global-tool\\b"
# strip_ansi = true
# strip_lines_matching = ["^\\s*$"]
# max_lines = 40
"#;

const TOK_MD: &str = "TOK.md";
const CLAUDE_MD: &str = "CLAUDE.md";
const AGENTS_MD: &str = "AGENTS.md";
const TOK_MD_REF: &str = "@TOK.md";
const GEMINI_MD: &str = "GEMINI.md";

/// Control flow for settings.json patching
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PatchMode {
    Ask,  // Default: prompt user [y/N]
    Auto, // --auto-patch: no prompt
    Skip, // --no-patch: manual instructions
}

/// Result of settings.json patching operation
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PatchResult {
    Patched,        // Hook was added successfully
    AlreadyPresent, // Hook was already in settings.json
    Declined,       // User declined when prompted
    Skipped,        // --no-patch flag used
}

// Legacy full instructions for backward compatibility (--claude-md mode)
const TOK_INSTRUCTIONS: &str = r##"<!-- tok-instructions v3 -->
# TOK (Token Optimization Kit) - Token-Optimized Commands

## Golden Rule

**Always prefix commands with `tok`**. If TOK has a dedicated filter, it uses it. If not, it passes through unchanged. This means TOK is always safe to use.

**Important**: Even in command chains with `&&`, use `tok`:
```bash
# ❌ Wrong
git add . && git commit -m "msg" && git push

# ✅ Correct
tok git add . && tok git commit -m "msg" && tok git push
```

## TOK Commands by Workflow

### Git (59-80% savings)
```bash
tok git status          # Compact status
tok git log             # Compact log (works with all git flags)
tok git diff            # Compact diff (80%)
tok git show            # Compact show (80%)
tok git add             # Ultra-compact confirmations (59%)
tok git commit          # Ultra-compact confirmations (59%)
tok git push            # Ultra-compact confirmations
tok git pull            # Ultra-compact confirmations
tok git branch          # Compact branch list
tok git fetch           # Compact fetch
tok git stash           # Compact stash
tok git worktree        # Compact worktree
```

Note: Git passthrough works for ALL subcommands, even those not explicitly listed.

### GitHub (26-87% savings)
```bash
tok gh pr view <num>    # Compact PR view (87%)
tok gh pr checks        # Compact PR checks (79%)
tok gh run list         # Compact workflow runs (82%)
tok gh issue list       # Compact issue list (80%)
tok gh api              # Compact API responses (26%)
```

### Build & Compile (80-90% savings)
```bash
tok cargo build         # Cargo build output
tok cargo check         # Cargo check output
tok cargo clippy        # Clippy warnings grouped by file (80%)
tok tsc                 # TypeScript errors grouped by file/code (83%)
tok lint                # ESLint/Biome violations grouped (84%)
tok prettier --check .  # Files needing format only (70%)
tok next build          # Next.js build with route metrics (87%)
tok dotnet build        # .NET build compact
tok dotnet restore      # NuGet without the scroll
tok go build            # Go build, errors loud, chatter low
tok go vet              # Go static checks, compact receipts
```

### Test (90-99% savings)
```bash
tok cargo test          # Cargo test failures only (90%)
tok cargo nextest       # Nextest parallel failures in focus
tok vitest run          # Vitest failures only (99.5%)
tok playwright test     # Playwright failures only (94%)
tok pytest              # Red tests first, fluff last (90%)
tok go test ./...       # Go test JSON stream, ~90% fewer tokens
tok rspec               # RSpec failures, not the whole sonnet
tok rake test           # Minitest without the wallpaper
tok dotnet test         # Xunit without the XML wall
tok test <cmd>          # Generic test wrapper — failures only
```

### JavaScript/TypeScript Tooling (70-90% savings)
```bash
tok pnpm list           # Compact dependency tree (70%)
tok pnpm outdated       # Compact outdated packages (80%)
tok pnpm install        # Compact install output (90%)
tok npm run <script>    # Compact npm script output
tok npx <cmd>           # Smart routing to tsc/eslint/prisma filters
tok prisma generate     # Client code, zero ASCII confetti (88%)
tok prisma migrate dev  # Schema travel log, compact
tok prisma db push      # "Just make it match" energy
```

### Python Tooling (80-90% savings)
```bash
tok ruff check .        # Python linting, compact
tok mypy .              # Type errors grouped for humans
tok pytest              # Red tests first, fluff last
tok pip install <pkg>   # pip/uv without the spam
tok pip list            # Compact package list
```

### Ruby Tooling (80-85% savings)
```bash
tok rake test           # Minitest without the wallpaper
tok rubocop             # RuboCop compact docket
tok rspec               # RSpec failures, not the whole sonnet
```

### Go Tooling (85-90% savings)
```bash
tok go test ./...       # JSON stream, ~90% fewer tokens
tok go build            # Errors loud, chatter low
tok go vet              # Static checks, compact receipts
tok golangci-lint run   # Many linters, one tight transcript
```

### Graphite / Stacked PRs (70% savings)
```bash
tok gt log              # Stack story, short chapters
tok gt submit           # Ship the stack, skip the soliloquy
tok gt sync             # Trunk + branches, tight summary
tok gt restack          # Replay commits, fewer lines
tok gt create           # New branch/stack slice, compact
tok gt branch           # Info and moves, Graphite-style
```

### Files & Search (60-75% savings)
```bash
tok ls <path>           # Tree format, compact (65%)
tok tree                # tree(1) you can scroll past
tok read <file>         # Code reading with filtering (60%)
tok smart <file>        # Two-line file summary (local, no cloud)
tok grep <pattern>      # Search grouped by file (75%)
tok find <pattern>      # Find grouped by directory (70%)
tok wc <file>           # Counts without decorative padding
```

### Analysis & Debug (70-90% savings)
```bash
tok err <cmd>           # Filter errors only from any command
tok log <file>          # Deduplicated logs with counts
tok json <file>         # JSON shrink values or --schema for shapes
tok deps                # Dependency overview
tok env                 # Environment variables compact, secrets hidden
tok summary <cmd>       # Heuristic summary of command output
tok diff                # Ultra-compact diffs
tok format              # Auto-picks prettier / black / ruff format
```

### Infrastructure (85% savings)
```bash
tok docker ps           # Compact container list
tok docker images       # Compact image list
tok docker logs <c>     # Deduplicated logs
tok docker compose ps   # Compose services at a glance
tok docker compose logs # Compose logs, dedupe magic
tok kubectl pods        # Who's up, who's pending
tok kubectl services    # Names and ports, trimmed
tok kubectl logs        # Pod gossip, condensed
```

### Cloud & Database (70-85% savings)
```bash
tok aws <cmd>           # AWS CLI JSON, human-sized lines out
tok psql <cmd>          # Tidy tables, fewer borders, fewer tokens
```

### Network (65-70% savings)
```bash
tok curl <url>          # JSON auto-detected; --schema for shapes (70%)
tok wget <url>          # Compact download output (65%)
```

### .NET (80-85% savings)
```bash
tok dotnet build        # MSBuild murmur, not shout
tok dotnet test         # Xunit without the XML wall
tok dotnet restore      # NuGet without the scroll
tok dotnet format       # dotnet-format, trimmed transcript
```

### Code Graph — reach for these BEFORE reading files
```bash
tok mem ask "<question>"  # The symbols that answer it, source inlined. Start here.
tok mem skeleton <file>   # Every signature in a file, no bodies (~10% of the tokens)
tok mem grep "<pattern>"  # Text search, each hit attributed to its enclosing symbol
tok mem map               # Repo layout, hubs, entry points — orientation in one screen
tok mem check             # Has the committed .tok/map/ drifted from the code?
```
Add `--in <path>` to `ask`/`grep` to stay inside one package of a monorepo.
The same six are on MCP as `tok_ask`, `tok_skeleton`, `tok_grep`, `tok_map`,
`tok_relations`, `tok_check`. The graph refreshes itself before each query, so
an edit made a second ago is already visible.

### Code Intelligence (use when needed)
```bash
tok mem index <dir>     # Index symbols, relationships, structure
tok mem search <query>  # Full-text search across indexed symbols (BM25)
tok mem find <symbol>   # Exact or fuzzy symbol lookup
tok mem context <symbol>  # Callers, callees, type refs
tok mem impact <symbol> # Blast radius — who breaks if this changes?
tok mem dead-code       # Symbols with zero inbound references
tok mem changes         # What changed since last session
tok mem detect          # Symbols affected by changed files
tok mem relations <sym> # Callers, callees, hierarchy, imports
tok mem evolution       # What changed in a time window
tok mem timeline <sym>  # Full change history of a symbol
tok mem central         # Most central symbols (highest connectivity)
tok mem bridges         # Bridge symbols connecting subgraphs
tok mem communities     # Connected components in symbol graph
tok mem complexity <fn> # Cyclomatic complexity estimate
tok mem repos           # List all indexed repositories
tok mem status          # Index stats and health
tok mem forget <repo>   # Remove indexed repo from memory DB
```

### Agent Memory (`tok memory` — not `tok mem`)
Rules, preferences, and project facts for LLM sessions. **Enabled by default** after `tok init -g`. Hooks inject at session start; auto-extract after turns.

```bash
tok memory status              # DB path, counts, enabled flags
tok memory on / off            # Enable or disable agent memory
tok memory extraction true     # Toggle auto-extraction (add after turns)
tok memory add "<text>"        # Store rule/preference (--type, --project)
tok memory search "<query>"    # Scoped search (BM25 + hybrid scoring)
tok memory list                # List memories (--type, --status)
tok memory show <id>           # Show one record
tok memory forget <id>         # Delete permanently
tok memory archive <id>        # Archive without deleting
tok memory reject <id>         # Mark rejected (won't inject)
tok memory inspect-context "<q>"  # Preview injected context pack
tok memory export --format json   # Backup vault (-o path)
tok memory import <file>       # Restore from export
tok memory clear --project     # Clear project-scoped memories
tok hook memory-retrieve --json   # sessionStart hook (Cursor, etc.)
tok hook memory-extract           # Post-turn extract (JSON stdin)
```

### ForgeMap
```bash
tok forgemap init       # Inject ForgeMap headers into source files
tok forgemap update     # Annotate only files missing a header
tok forgemap check      # Coverage report — exit 1 if unannotated
tok forgemap refresh    # Update exports:/used_by: only
tok forgemap manifest   # Generate .forgemap project manifest
tok forgemap wiki bootstrap  # Emit per-file Obsidian vault
tok forgemap wiki sync  # Regenerate narrative project wiki
```

### Security
```bash
tok --security <cmd>    # Obfuscate sensitive data in output
tok security-inspect <text>  # Dry-run: inspect text for secrets
tok doctor --slm        # Check SLM runtime health and configuration
```

Global security flags: `--security`, `--no-security`, `--security-mode <mode>` (observe/balanced/strict/developer), `--slm`, `--no-slm`

### Analytics & Insights
```bash
tok gain                # Token savings dashboard
tok gain --graph        # ASCII graph of daily savings
tok gain --history      # Per-command savings history
tok cc-economics        # Claude spend vs tok savings — receipts included
tok discover            # Find missed TOK opportunities in session history
tok session             # Usage stats across sessions
tok learn               # Learn CLI fixes from past mistakes
```

### Configuration & Debugging
```bash
tok config              # View or scaffold tok config
tok verify              # Sanity-check hooks + run inline TOML filter tests
tok trust               # Trust this project's .tok filter recipes
tok untrust             # Remove trusted local TOML filters
tok proxy <cmd>         # Raw command passthrough (still tracks stats)
tok man                 # Full command manual (every command with descriptions)
tok man <topic>         # Filter manual to a topic (e.g. tok man security)
```

### Useful Flags
- `-u` / `--ultra-compact` — maximum compression (ASCII icons, inline fields)
- `--security` / `--security-mode <mode>` — privacy/obfuscation layer
- `-v` / `-vv` / `-vvv` — increase verbosity

## Token Savings Overview

| Category | Commands | Typical Savings |
|----------|----------|-----------------|
| Tests | vitest, playwright, cargo test, pytest, go test, rspec | 90-99% |
| Build | next, tsc, lint, prettier, cargo, dotnet, go | 70-90% |
| Git | status, log, diff, add, commit, push, pull | 59-80% |
| GitHub | gh pr, gh run, gh issue, gh api | 26-87% |
| Package Managers | pnpm, npm, npx, pip | 70-90% |
| Files | ls, tree, read, smart, grep, find, wc | 60-75% |
| Linting | ruff, mypy, rubocop, golangci-lint, format | 80-90% |
| Infrastructure | docker, kubectl | 85% |
| Cloud/DB | aws, psql | 70-85% |
| Network | curl, wget | 65-70% |
| Graphite | gt log, submit, sync, restack | 70% |

Overall average: **60-90% token reduction** on common development operations.
<!-- /tok-instructions -->
"##;

/// Main entry point for `tok init`
#[allow(clippy::too_many_arguments)]
pub fn run(
    global: bool,
    install_claude: bool,
    install_opencode: bool,
    install_cursor: bool,
    install_windsurf: bool,
    install_cline: bool,
    claude_md: bool,
    hook_only: bool,
    codex: bool,
    patch_mode: PatchMode,
    verbose: u8,
    graph: bool,
) -> Result<()> {
    // Validation: Codex mode conflicts
    if codex {
        if install_opencode {
            anyhow::bail!("--codex cannot be combined with --opencode");
        }
        if claude_md {
            anyhow::bail!("--codex cannot be combined with --claude-md");
        }
        if hook_only {
            anyhow::bail!("--codex cannot be combined with --hook-only");
        }
        if matches!(patch_mode, PatchMode::Auto) {
            anyhow::bail!("--codex cannot be combined with --auto-patch");
        }
        if matches!(patch_mode, PatchMode::Skip) {
            anyhow::bail!("--codex cannot be combined with --no-patch");
        }
        return run_codex_mode(global, verbose);
    }

    // Validation: Global-only features
    if install_opencode && !global {
        anyhow::bail!("OpenCode plugin is global-only. Use: tok init -g --opencode");
    }

    if install_cursor && !global {
        anyhow::bail!("Cursor hooks are global-only. Use: tok init -g --agent cursor");
    }

    if install_windsurf && !global {
        anyhow::bail!("Windsurf support is global-only. Use: tok init -g --agent windsurf");
    }

    // Windsurf-only mode
    if install_windsurf {
        return run_windsurf_mode(verbose);
    }

    // Cline-only mode
    if install_cline {
        return run_cline_mode(verbose);
    }

    // Mode selection (Claude Code / OpenCode)
    match (install_claude, install_opencode, claude_md, hook_only) {
        (false, true, _, _) => run_opencode_only_mode(verbose)?,
        (true, opencode, true, _) => run_claude_md_mode(global, verbose, opencode)?,
        (true, opencode, false, true) => {
            run_hook_only_mode(global, patch_mode, verbose, opencode, graph)?
        }
        (true, opencode, false, false) => {
            run_default_mode(global, patch_mode, verbose, opencode, graph)?
        }
        (false, false, _, _) => {
            if !install_cursor {
                anyhow::bail!("at least one of install_claude or install_opencode must be true")
            }
        }
    }

    // Cursor hooks (additive, installed alongside Claude Code)
    if install_cursor {
        install_cursor_hooks(verbose, graph)?;
    }

    if global {
        if let Err(e) = crate::agent_memory::service::ensure_on_init() {
            if verbose > 0 {
                eprintln!("[warn] Agent memory setup: {e}");
            }
        } else if verbose > 0 {
            println!("[ok] Agent memory enabled (tok memory)");
        }
    }

    println!();

    Ok(())
}

/// Prepare hook directory and return paths (hook_dir, hook_path)
fn prepare_hook_paths() -> Result<(PathBuf, PathBuf)> {
    let claude_dir = resolve_claude_dir()?;
    let hook_dir = claude_dir.join("hooks");
    fs::create_dir_all(&hook_dir)
        .with_context(|| format!("Failed to create hook directory: {}", hook_dir.display()))?;
    let hook_path = hook_dir.join(REWRITE_HOOK_FILE);
    Ok((hook_dir, hook_path))
}

/// Write hook file if missing or outdated, return true if changed
#[cfg(unix)]
fn ensure_hook_installed(hook_path: &Path, verbose: u8) -> Result<bool> {
    let changed = if hook_path.exists() {
        let existing = fs::read_to_string(hook_path)
            .with_context(|| format!("Failed to read existing hook: {}", hook_path.display()))?;

        if existing == REWRITE_HOOK {
            if verbose > 0 {
                eprintln!("Hook already up to date: {}", hook_path.display());
            }
            false
        } else {
            fs::write(hook_path, REWRITE_HOOK)
                .with_context(|| format!("Failed to write hook to {}", hook_path.display()))?;
            if verbose > 0 {
                eprintln!("Updated hook: {}", hook_path.display());
            }
            true
        }
    } else {
        fs::write(hook_path, REWRITE_HOOK)
            .with_context(|| format!("Failed to write hook to {}", hook_path.display()))?;
        if verbose > 0 {
            eprintln!("Created hook: {}", hook_path.display());
        }
        true
    };

    // Set executable permissions
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(hook_path, fs::Permissions::from_mode(0o755))
        .with_context(|| format!("Failed to set hook permissions: {}", hook_path.display()))?;

    // Store SHA-256 hash for runtime integrity verification.
    // Always store (idempotent) to ensure baseline exists even for
    // hooks installed before integrity checks were added.
    integrity::store_hash(hook_path)
        .with_context(|| format!("Failed to store integrity hash for {}", hook_path.display()))?;
    if verbose > 0 && changed {
        eprintln!("Stored integrity hash for hook");
    }

    Ok(changed)
}

/// Idempotent file write: create or update if content differs
fn write_if_changed(path: &Path, content: &str, name: &str, verbose: u8) -> Result<bool> {
    if path.exists() {
        let existing = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}: {}", name, path.display()))?;

        if existing == content {
            if verbose > 0 {
                eprintln!("{} already up to date: {}", name, path.display());
            }
            Ok(false)
        } else {
            fs::write(path, content)
                .with_context(|| format!("Failed to write {}: {}", name, path.display()))?;
            if verbose > 0 {
                eprintln!("Updated {}: {}", name, path.display());
            }
            Ok(true)
        }
    } else {
        fs::write(path, content)
            .with_context(|| format!("Failed to write {}: {}", name, path.display()))?;
        if verbose > 0 {
            eprintln!("Created {}: {}", name, path.display());
        }
        Ok(true)
    }
}

/// Atomic write using tempfile + rename
/// Prevents corruption on crash/interrupt
fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let parent = path.parent().with_context(|| {
        format!(
            "Cannot write to {}: path has no parent directory",
            path.display()
        )
    })?;

    // Create temp file in same directory (ensures same filesystem for atomic rename)
    let mut temp_file = NamedTempFile::new_in(parent)
        .with_context(|| format!("Failed to create temp file in {}", parent.display()))?;

    // Write content
    temp_file
        .write_all(content.as_bytes())
        .with_context(|| format!("Failed to write {} bytes to temp file", content.len()))?;

    // Atomic rename
    temp_file.persist(path).with_context(|| {
        format!(
            "Failed to atomically replace {} (disk full?)",
            path.display()
        )
    })?;

    Ok(())
}

/// Prompt user for consent to patch settings.json
/// Prints to stderr (stdout may be piped), reads from stdin
/// Default is No (capital N)
fn prompt_user_consent(settings_path: &Path) -> Result<bool> {
    use std::io::{self, BufRead, IsTerminal};

    eprintln!("\nPatch existing {}? [y/N] ", settings_path.display());

    // If stdin is not a terminal (piped), default to No
    if !io::stdin().is_terminal() {
        eprintln!("(non-interactive mode, defaulting to N)");
        return Ok(false);
    }

    let stdin = io::stdin();
    let mut line = String::new();
    stdin
        .lock()
        .read_line(&mut line)
        .context("Failed to read user input")?;

    let response = line.trim().to_lowercase();
    Ok(response == "y" || response == "yes")
}

/// Print manual instructions for settings.json patching
fn print_manual_instructions(hook_path: &Path, include_opencode: bool) {
    println!("\n  MANUAL STEP: Add this to ~/.claude/settings.json:");
    println!("  {{");
    println!("    \"hooks\": {{ \"PreToolUse\": [{{");
    println!("      \"matcher\": \"Bash\",");
    println!("      \"hooks\": [{{ \"type\": \"command\",");
    println!("        \"command\": \"{}\"", hook_path.display());
    println!("      }}]");
    println!("    }}]}}");
    println!("  }}");
    if include_opencode {
        println!("\n  Then restart Claude Code + OpenCode and run `git status` to sanity-check.\n");
    } else {
        println!("\n  Then restart Claude Code and run `git status` to sanity-check.\n");
    }
}

fn remove_hook_from_json(root: &mut serde_json::Value) -> bool {
    let hooks = match root
        .get_mut("hooks")
        .and_then(|h| h.get_mut(PRE_TOOL_USE_KEY))
    {
        Some(pre_tool_use) => pre_tool_use,
        None => return false,
    };

    let pre_tool_use_array = match hooks.as_array_mut() {
        Some(arr) => arr,
        None => return false,
    };

    let original_len = pre_tool_use_array.len();
    pre_tool_use_array.retain(|entry| {
        if let Some(hooks_array) = entry.get("hooks").and_then(|h| h.as_array()) {
            for hook in hooks_array {
                if let Some(command) = hook.get("command").and_then(|c| c.as_str()) {
                    if command.contains(REWRITE_HOOK_FILE) {
                        return false;
                    }
                }
            }
        }
        true
    });

    pre_tool_use_array.len() < original_len
}

/// Remove TOK hook from settings.json file
/// Backs up before modification, returns true if hook was found and removed
fn remove_hook_from_settings(verbose: u8) -> Result<bool> {
    let claude_dir = resolve_claude_dir()?;
    let settings_path = claude_dir.join(SETTINGS_JSON);

    if !settings_path.exists() {
        if verbose > 0 {
            eprintln!("settings.json not found, nothing to remove");
        }
        return Ok(false);
    }

    let content = fs::read_to_string(&settings_path)
        .with_context(|| format!("Failed to read {}", settings_path.display()))?;

    if content.trim().is_empty() {
        return Ok(false);
    }

    let mut root: serde_json::Value = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse {} as JSON", settings_path.display()))?;

    let removed_rewrite = remove_hook_from_json(&mut root);
    let removed_memory = remove_memory_hooks_from_claude_json(&mut root);
    let removed_graph = remove_graph_hooks_from_claude_json(&mut root);
    let removed = removed_rewrite || removed_memory || removed_graph;

    if removed {
        // Backup original
        let backup_path = settings_path.with_extension("json.bak");
        fs::copy(&settings_path, &backup_path)
            .with_context(|| format!("Failed to backup to {}", backup_path.display()))?;

        // Atomic write
        let serialized =
            serde_json::to_string_pretty(&root).context("Failed to serialize settings.json")?;
        atomic_write(&settings_path, &serialized)?;

        if verbose > 0 {
            eprintln!("Removed TOK hook from settings.json");
        }
    }

    Ok(removed)
}

/// Full uninstall for Claude, Gemini, Codex, or Cursor artifacts.
pub fn uninstall(global: bool, gemini: bool, codex: bool, cursor: bool, verbose: u8) -> Result<()> {
    if codex {
        return uninstall_codex(global, verbose);
    }

    if cursor {
        if !global {
            anyhow::bail!("Cursor uninstall only works with --global flag");
        }
        let cursor_removed =
            remove_cursor_hooks(verbose).context("Failed to remove Cursor hooks")?;
        if !cursor_removed.is_empty() {
            println!("TOK uninstalled (Cursor):");
            for item in &cursor_removed {
                println!("  - {}", item);
            }
            println!("\nRestart Cursor to apply changes.");
        } else {
            println!("TOK Cursor support was not installed (nothing to remove)");
        }
        return Ok(());
    }

    if !global {
        anyhow::bail!("Uninstall only works with --global flag. For local projects, manually remove TOK from CLAUDE.md");
    }

    let claude_dir = resolve_claude_dir()?;
    let mut removed = Vec::new();

    // Also uninstall Gemini artifacts if --gemini or always (clean everything)
    if gemini {
        let gemini_removed = uninstall_gemini(verbose)?;
        removed.extend(gemini_removed);
        if !removed.is_empty() {
            println!("TOK uninstalled (Gemini):");
            for item in &removed {
                println!("  - {}", item);
            }
            println!("\nRestart Gemini CLI to apply changes.");
        } else {
            println!("TOK Gemini support was not installed (nothing to remove)");
        }
        return Ok(());
    }

    // 1. Remove hook file
    let hook_path = claude_dir.join(HOOKS_SUBDIR).join(REWRITE_HOOK_FILE);
    if hook_path.exists() {
        fs::remove_file(&hook_path)
            .with_context(|| format!("Failed to remove hook: {}", hook_path.display()))?;
        removed.push(format!("Hook: {}", hook_path.display()));
    }

    // 1b. Remove integrity hash file
    if integrity::remove_hash(&hook_path)? {
        removed.push("Integrity hash: removed".to_string());
    }

    // 2. Remove TOK.md
    let tok_md_path = claude_dir.join(TOK_MD);
    if tok_md_path.exists() {
        fs::remove_file(&tok_md_path)
            .with_context(|| format!("Failed to remove TOK.md: {}", tok_md_path.display()))?;
        removed.push(format!("TOK.md: {}", tok_md_path.display()));
    }

    // 3. Remove @TOK.md reference from CLAUDE.md
    let claude_md_path = claude_dir.join(CLAUDE_MD);
    if claude_md_path.exists() {
        let content = fs::read_to_string(&claude_md_path)
            .with_context(|| format!("Failed to read CLAUDE.md: {}", claude_md_path.display()))?;

        if content.contains(TOK_MD_REF) {
            let new_content = content
                .lines()
                .filter(|line| !line.trim().starts_with(TOK_MD_REF))
                .collect::<Vec<_>>()
                .join("\n");

            // Clean up double blanks
            let cleaned = clean_double_blanks(&new_content);

            fs::write(&claude_md_path, cleaned).with_context(|| {
                format!("Failed to write CLAUDE.md: {}", claude_md_path.display())
            })?;
            removed.push("CLAUDE.md: removed @TOK.md reference".to_string());
        }
    }

    // 4. Remove hook entry from settings.json
    if remove_hook_from_settings(verbose)? {
        removed.push("settings.json: removed TOK hook entry".to_string());
    }

    // 5. Remove OpenCode plugin
    let opencode_removed = remove_opencode_plugin(verbose)?;
    for path in opencode_removed {
        removed.push(format!("OpenCode plugin: {}", path.display()));
    }

    // 6. Remove Cursor hooks
    let cursor_removed = remove_cursor_hooks(verbose)?;
    removed.extend(cursor_removed);

    // Report results
    if removed.is_empty() {
        println!("TOK was not installed (nothing to remove)");
    } else {
        println!("TOK uninstalled:");
        for item in removed {
            println!("  - {}", item);
        }
        println!("\nRestart Claude Code, OpenCode, and Cursor (if you use them) so the hooks actually reload.");
    }

    Ok(())
}

fn uninstall_codex(global: bool, verbose: u8) -> Result<()> {
    if !global {
        anyhow::bail!(
            "Uninstall only works with --global flag. For local projects, manually remove TOK from AGENTS.md"
        );
    }

    let codex_dir = resolve_codex_dir()?;
    let removed = uninstall_codex_at(&codex_dir, verbose)?;

    if removed.is_empty() {
        println!("TOK was not installed for Codex CLI (nothing to remove)");
    } else {
        println!("TOK uninstalled for Codex CLI:");
        for item in removed {
            println!("  - {}", item);
        }
    }

    Ok(())
}

fn uninstall_codex_at(codex_dir: &Path, verbose: u8) -> Result<Vec<String>> {
    let mut removed = Vec::new();

    let tok_md_path = codex_dir.join(TOK_MD);
    if tok_md_path.exists() {
        fs::remove_file(&tok_md_path)
            .with_context(|| format!("Failed to remove TOK.md: {}", tok_md_path.display()))?;
        if verbose > 0 {
            eprintln!("Removed TOK.md: {}", tok_md_path.display());
        }
        removed.push(format!("TOK.md: {}", tok_md_path.display()));
    }

    let agents_md_path = codex_dir.join(AGENTS_MD);
    if remove_tok_reference_from_agents(&agents_md_path, verbose)? {
        removed.push("AGENTS.md: removed @TOK.md reference".to_string());
    }

    Ok(removed)
}

/// Which families of hook a reconciliation had to add.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct AddedHooks {
    rewrite: bool,
    memory: bool,
    graph: bool,
}

impl AddedHooks {
    fn any(self) -> bool {
        self.rewrite || self.memory || self.graph
    }
}

/// Bring a Claude `settings.json` up to the hook set this tok build installs.
///
/// Every family is checked independently, because the presence of one says
/// nothing about the others: an install predating the memory or graph hooks has
/// the rewrite hook and neither of theirs, and treating that as "already
/// configured" would leave it upgraded in binary and not in configuration.
///
/// Mutates `root` and reports what it added; writes nothing.
fn reconcile_claude_hooks(
    root: &mut serde_json::Value,
    hook_command: &str,
    hooks_dir: Option<&Path>,
    graph: bool,
) -> AddedHooks {
    let mut added = AddedHooks::default();

    if !hook_already_present(root, hook_command) {
        insert_hook_entry(root, hook_command);
        added.rewrite = true;
    }

    if let Some(dir) = hooks_dir {
        added.memory = patch_claude_memory_hooks(root, dir);
        if graph {
            added.graph = patch_claude_graph_hooks(root, dir);
        }
    }

    added
}

/// Orchestrator: patch settings.json with TOK hook
/// Handles reading, checking, prompting, merging, backing up, and atomic writing
fn patch_settings_json(
    hook_path: &Path,
    mode: PatchMode,
    verbose: u8,
    include_opencode: bool,
    graph: bool,
) -> Result<PatchResult> {
    let claude_dir = resolve_claude_dir()?;
    let settings_path = claude_dir.join(SETTINGS_JSON);
    let hook_command = hook_path
        .to_str()
        .context("Hook path contains invalid UTF-8")?;

    // Read or create settings.json
    let mut root = if settings_path.exists() {
        let content = fs::read_to_string(&settings_path)
            .with_context(|| format!("Failed to read {}", settings_path.display()))?;

        if content.trim().is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str(&content)
                .with_context(|| format!("Failed to parse {} as JSON", settings_path.display()))?
        }
    } else {
        serde_json::json!({})
    };

    // Work out every change on a copy first, so consent is asked for only when
    // there is something to consent to.
    let mut updated = root.clone();
    let hooks_dir = hook_path.parent();
    let added = reconcile_claude_hooks(&mut updated, hook_command, hooks_dir, graph);

    if !added.any() {
        if verbose > 0 {
            eprintln!("settings.json: hooks already present");
        }
        return Ok(PatchResult::AlreadyPresent);
    }

    // Handle mode
    match mode {
        PatchMode::Skip => {
            print_manual_instructions(hook_path, include_opencode);
            return Ok(PatchResult::Skipped);
        }
        PatchMode::Ask => {
            if !prompt_user_consent(&settings_path)? {
                print_manual_instructions(hook_path, include_opencode);
                return Ok(PatchResult::Declined);
            }
        }
        PatchMode::Auto => {
            // Proceed without prompting
        }
    }

    // Only now that the change is agreed to does anything reach the disk. The
    // scripts are written unconditionally rather than only when newly
    // registered, so an existing install picks up their current contents.
    if let Some(dir) = hooks_dir {
        install_memory_hook_scripts(dir, "claude", verbose)?;
        if added.memory && verbose > 0 {
            eprintln!("settings.json: agent memory hooks added");
        }

        if graph {
            install_graph_hook_scripts(dir, "claude", verbose)?;
            if added.graph && verbose > 0 {
                eprintln!("settings.json: code graph hooks added");
            }
        }
    }

    root = updated;

    // Backup original
    if settings_path.exists() {
        let backup_path = settings_path.with_extension("json.bak");
        fs::copy(&settings_path, &backup_path)
            .with_context(|| format!("Failed to backup to {}", backup_path.display()))?;
        if verbose > 0 {
            eprintln!("Backup: {}", backup_path.display());
        }
    }

    // Atomic write
    let serialized =
        serde_json::to_string_pretty(&root).context("Failed to serialize settings.json")?;
    atomic_write(&settings_path, &serialized)?;

    println!("\n  settings.json: hooks updated");
    if settings_path.with_extension("json.bak").exists() {
        println!(
            "  Backup: {}",
            settings_path.with_extension("json.bak").display()
        );
    }
    if include_opencode {
        println!("  Restart Claude Code + OpenCode, then poke `git status` to feel it.");
    } else {
        println!("  Restart Claude Code, then run `git status` for a quick vibe check.");
    }

    Ok(PatchResult::Patched)
}

/// Clean up consecutive blank lines (collapse 3+ to 2)
/// Used when removing @TOK.md line from CLAUDE.md
fn clean_double_blanks(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut result = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];

        if line.trim().is_empty() {
            // Count consecutive blank lines
            let mut blank_count = 0;
            while i < lines.len() && lines[i].trim().is_empty() {
                blank_count += 1;
                i += 1;
            }

            // Keep at most 2 blank lines
            let keep = blank_count.min(2);
            result.extend(std::iter::repeat_n("", keep));
        } else {
            result.push(line);
            i += 1;
        }
    }

    result.join("\n")
}

/// Deep-merge TOK hook entry into settings.json
/// Creates hooks.PreToolUse structure if missing, preserves existing hooks
fn insert_hook_entry(root: &mut serde_json::Value, hook_command: &str) {
    // Ensure root is an object
    let root_obj = match root.as_object_mut() {
        Some(obj) => obj,
        None => {
            *root = serde_json::json!({});
            root.as_object_mut()
                .expect("Just created object, must succeed")
        }
    };

    // Use entry() API for idiomatic insertion
    let hooks = root_obj
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .expect("hooks must be an object");

    let pre_tool_use = hooks
        .entry(PRE_TOOL_USE_KEY)
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .expect("PreToolUse must be an array");

    // Append TOK hook entry
    pre_tool_use.push(serde_json::json!({
        "matcher": "Bash",
        "hooks": [{
            "type": "command",
            "command": hook_command
        }]
    }));
}

/// Write shared memory hook scripts with a fixed `TOK_CLIENT`.
#[cfg(unix)]
fn install_memory_hook_scripts(hooks_dir: &Path, client: &str, verbose: u8) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let scripts = [
        (MEMORY_SESSION_HOOK_FILE, MEMORY_SESSION_HOOK),
        (MEMORY_PROMPT_HOOK_FILE, MEMORY_PROMPT_HOOK),
        (MEMORY_EXTRACT_HOOK_FILE, MEMORY_EXTRACT_HOOK),
    ];

    for (name, template) in scripts {
        let path = hooks_dir.join(name);
        let content = render_memory_hook_script(template, client);
        write_if_changed(&path, &content, name, verbose)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .with_context(|| format!("Failed to set permissions on {}", path.display()))?;
    }

    if client == "cursor" {
        let path = hooks_dir.join(MEMORY_CACHE_PROMPT_HOOK_FILE);
        write_if_changed(
            &path,
            MEMORY_CACHE_PROMPT_HOOK,
            MEMORY_CACHE_PROMPT_HOOK_FILE,
            verbose,
        )?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .with_context(|| format!("Failed to set permissions on {}", path.display()))?;
    }

    Ok(())
}

#[cfg(not(unix))]
fn install_memory_hook_scripts(_hooks_dir: &Path, _client: &str, _verbose: u8) -> Result<()> {
    Ok(())
}

fn render_memory_hook_script(template: &str, client: &str) -> String {
    let mut out = String::new();
    for line in template.lines() {
        out.push_str(line);
        out.push('\n');
        if line.starts_with("#!") {
            out.push_str(&format!("export TOK_CLIENT={client}\n"));
        }
    }
    out
}

/// Add Claude Code memory hooks (SessionStart, UserPromptSubmit, Stop).
fn patch_claude_memory_hooks(root: &mut serde_json::Value, hooks_dir: &Path) -> bool {
    let session = hooks_dir.join(MEMORY_SESSION_HOOK_FILE);
    let prompt = hooks_dir.join(MEMORY_PROMPT_HOOK_FILE);
    let extract = hooks_dir.join(MEMORY_EXTRACT_HOOK_FILE);

    let mut changed = false;
    changed |= insert_claude_hook_command(root, SESSION_START_KEY, &session);
    changed |= insert_claude_hook_command(root, USER_PROMPT_SUBMIT_KEY, &prompt);
    changed |= insert_claude_hook_command(root, STOP_KEY, &extract);
    changed
}

fn insert_claude_hook_command(root: &mut serde_json::Value, event: &str, command: &Path) -> bool {
    insert_claude_hook_entry(root, event, command, MEMORY_HOOK_MARKER, None)
}

/// Insert a hook command, skipping it if one bearing `marker` is already
/// registered for the event.
///
/// `matcher` restricts which tools trigger the hook. Claude treats an absent
/// matcher as "every tool", which is right for lifecycle events and wrong for
/// anything that reacts to a specific one.
fn insert_claude_hook_entry(
    root: &mut serde_json::Value,
    event: &str,
    command: &Path,
    marker: &str,
    matcher: Option<&str>,
) -> bool {
    if claude_hook_command_present(root, event, marker) {
        return false;
    }

    let Some(cmd) = command.to_str() else {
        return false;
    };

    let root_obj = match root.as_object_mut() {
        Some(obj) => obj,
        None => {
            *root = serde_json::json!({});
            root.as_object_mut().expect("root object")
        }
    };

    let hooks = root_obj
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .expect("hooks object");

    let event_arr = hooks
        .entry(event)
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .expect("hook event array");

    let mut entry = serde_json::json!({
        "hooks": [{
            "type": "command",
            "command": cmd
        }]
    });

    if let Some(matcher) = matcher {
        entry["matcher"] = serde_json::json!(matcher);
    }

    event_arr.push(entry);

    true
}

/// Install the graph lifecycle hook scripts next to the other TOK hooks.
#[cfg(unix)]
fn install_graph_hook_scripts(hooks_dir: &Path, client: &str, verbose: u8) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let scripts = [
        (GRAPH_SESSION_HOOK_FILE, GRAPH_SESSION_HOOK),
        (GRAPH_POSTEDIT_HOOK_FILE, GRAPH_POSTEDIT_HOOK),
    ];

    for (name, template) in scripts {
        let path = hooks_dir.join(name);
        let content = render_memory_hook_script(template, client);
        write_if_changed(&path, &content, name, verbose)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .with_context(|| format!("Failed to set permissions on {}", path.display()))?;
    }

    Ok(())
}

#[cfg(not(unix))]
fn install_graph_hook_scripts(_hooks_dir: &Path, _client: &str, _verbose: u8) -> Result<()> {
    Ok(())
}

/// Add Claude Code graph hooks: orientation at session start, refresh after an
/// edit.
fn patch_claude_graph_hooks(root: &mut serde_json::Value, hooks_dir: &Path) -> bool {
    let session = hooks_dir.join(GRAPH_SESSION_HOOK_FILE);
    let postedit = hooks_dir.join(GRAPH_POSTEDIT_HOOK_FILE);

    let mut changed = false;
    changed |= insert_claude_hook_entry(root, SESSION_START_KEY, &session, GRAPH_HOOK_MARKER, None);
    changed |= insert_claude_hook_entry(
        root,
        POST_TOOL_USE_KEY,
        &postedit,
        GRAPH_HOOK_MARKER,
        Some(EDIT_TOOL_MATCHER),
    );
    changed
}

/// Remove the graph hooks, leaving every other hook in place.
fn remove_graph_hooks_from_claude_json(root: &mut serde_json::Value) -> bool {
    let mut any = false;
    for event in [SESSION_START_KEY, POST_TOOL_USE_KEY] {
        let Some(arr) = root
            .pointer_mut(&format!("/hooks/{event}"))
            .and_then(|v| v.as_array_mut())
        else {
            continue;
        };

        let before = arr.len();
        arr.retain(|entry| {
            !entry
                .get("hooks")
                .and_then(|h| h.as_array())
                .into_iter()
                .flatten()
                .filter_map(|hook| hook.get("command").and_then(|c| c.as_str()))
                .any(|cmd| cmd.contains(GRAPH_HOOK_MARKER))
        });

        any |= arr.len() < before;
    }

    any
}

fn claude_hook_command_present(root: &serde_json::Value, event: &str, needle: &str) -> bool {
    root.get("hooks")
        .and_then(|h| h.get(event))
        .and_then(|p| p.as_array())
        .is_some_and(|arr| {
            arr.iter()
                .filter_map(|entry| entry.get("hooks")?.as_array())
                .flatten()
                .filter_map(|hook| hook.get("command")?.as_str())
                .any(|cmd| cmd.contains(needle))
        })
}

fn remove_memory_hooks_from_claude_json(root: &mut serde_json::Value) -> bool {
    let events = [SESSION_START_KEY, USER_PROMPT_SUBMIT_KEY, STOP_KEY];
    let mut any = false;
    for event in events {
        let Some(arr) = root
            .pointer_mut(&format!("/hooks/{event}"))
            .and_then(|v| v.as_array_mut())
        else {
            continue;
        };
        let before = arr.len();
        arr.retain(|entry| {
            let has_memory = entry
                .get("hooks")
                .and_then(|h| h.as_array())
                .into_iter()
                .flatten()
                .filter_map(|hook| hook.get("command").and_then(|c| c.as_str()))
                .any(|cmd| cmd.contains(MEMORY_HOOK_MARKER));
            !has_memory
        });
        if arr.len() < before {
            any = true;
        }
    }
    any
}

/// Add Gemini CLI memory hooks (SessionStart, BeforeAgent, AfterAgent).
fn patch_gemini_memory_hooks(settings: &mut serde_json::Value, hooks_dir: &Path) -> Result<bool> {
    install_memory_hook_scripts(hooks_dir, "gemini", 0)?;

    let session = hooks_dir.join(MEMORY_SESSION_HOOK_FILE);
    let prompt = hooks_dir.join(MEMORY_PROMPT_HOOK_FILE);
    let extract = hooks_dir.join(MEMORY_EXTRACT_HOOK_FILE);

    let mut changed = false;
    changed |= insert_gemini_hook_command(settings, SESSION_START_KEY, &session, None);
    changed |= insert_gemini_hook_command(settings, BEFORE_AGENT_KEY, &prompt, None);
    changed |= insert_gemini_hook_command(settings, AFTER_AGENT_KEY, &extract, None);
    Ok(changed)
}

fn insert_gemini_hook_command(
    settings: &mut serde_json::Value,
    event: &str,
    command: &Path,
    matcher: Option<&str>,
) -> bool {
    if gemini_hook_command_present(settings, event, MEMORY_HOOK_MARKER) {
        return false;
    }

    let cmd = command
        .to_str()
        .expect("memory hook path must be valid UTF-8");

    let mut entry = serde_json::json!({
        "hooks": [{
            "type": "command",
            "command": cmd
        }]
    });
    if let Some(m) = matcher {
        entry["matcher"] = serde_json::json!(m);
    }

    let hooks = settings
        .as_object_mut()
        .expect("settings object")
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .expect("hooks object");

    let event_arr = hooks
        .entry(event)
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .expect("event array");

    event_arr.push(entry);
    true
}

fn gemini_hook_command_present(settings: &serde_json::Value, event: &str, needle: &str) -> bool {
    settings
        .pointer(&format!("/hooks/{event}"))
        .and_then(|v| v.as_array())
        .is_some_and(|arr| {
            arr.iter().any(|h| {
                h.pointer("/hooks/0/command")
                    .and_then(|c| c.as_str())
                    .is_some_and(|cmd| cmd.contains(needle))
            })
        })
}

fn remove_memory_hooks_from_gemini_json(settings: &mut serde_json::Value) -> bool {
    let events = [SESSION_START_KEY, BEFORE_AGENT_KEY, AFTER_AGENT_KEY];
    let mut any = false;
    for event in events {
        let pointer = format!("/hooks/{event}");
        let Some(arr) = settings
            .pointer_mut(&pointer)
            .and_then(|v| v.as_array_mut())
        else {
            continue;
        };
        let before = arr.len();
        arr.retain(|h| {
            !h.pointer("/hooks/0/command")
                .and_then(|c| c.as_str())
                .is_some_and(|cmd| cmd.contains(MEMORY_HOOK_MARKER))
        });
        if arr.len() < before {
            any = true;
        }
    }
    any
}

/// Check if TOK hook is already present in settings.json
/// Matches on tok-rewrite.sh substring to handle different path formats
fn hook_already_present(root: &serde_json::Value, hook_command: &str) -> bool {
    let pre_tool_use_array = match root
        .get("hooks")
        .and_then(|h| h.get(PRE_TOOL_USE_KEY))
        .and_then(|p| p.as_array())
    {
        Some(arr) => arr,
        None => return false,
    };

    pre_tool_use_array
        .iter()
        .filter_map(|entry| entry.get("hooks")?.as_array())
        .flatten()
        .filter_map(|hook| hook.get("command")?.as_str())
        .any(|cmd| {
            cmd == hook_command
                || (cmd.contains(REWRITE_HOOK_FILE) && hook_command.contains(REWRITE_HOOK_FILE))
        })
}

/// Default mode: hook + slim TOK.md + @TOK.md reference
#[cfg(not(unix))]
fn run_default_mode(
    _global: bool,
    _patch_mode: PatchMode,
    _verbose: u8,
    _install_opencode: bool,
    _graph: bool,
) -> Result<()> {
    eprintln!("[warn] Hook-based mode requires Unix (macOS/Linux).");
    eprintln!("    Windows: use --claude-md mode for full injection.");
    eprintln!("    Falling back to --claude-md mode.");
    run_claude_md_mode(_global, _verbose, _install_opencode)
}

#[cfg(unix)]
fn run_default_mode(
    global: bool,
    patch_mode: PatchMode,
    verbose: u8,
    install_opencode: bool,
    graph: bool,
) -> Result<()> {
    if !global {
        // Local init: inject CLAUDE.md + generate project-local filters template
        run_claude_md_mode(false, verbose, install_opencode)?;
        generate_project_filters_template(verbose)?;
        return Ok(());
    }

    let claude_dir = resolve_claude_dir()?;
    let tok_md_path = claude_dir.join(TOK_MD);
    let claude_md_path = claude_dir.join(CLAUDE_MD);

    // 1. Prepare hook directory and install hook
    let (_hook_dir, hook_path) = prepare_hook_paths()?;
    let hook_changed = ensure_hook_installed(&hook_path, verbose)?;

    // 2. Write TOK.md
    write_if_changed(&tok_md_path, TOK_SLIM, TOK_MD, verbose)?;

    let opencode_plugin_path = if install_opencode {
        let path = prepare_opencode_plugin_path()?;
        ensure_opencode_plugin_installed(&path, verbose)?;
        Some(path)
    } else {
        None
    };

    // 3. Patch CLAUDE.md (add @TOK.md, migrate if needed)
    let migrated = patch_claude_md(&claude_md_path, verbose)?;

    // 4. Print success message
    let hook_status = if hook_changed {
        "installed/updated"
    } else {
        "already up to date"
    };
    println!("\nNice — TOK hook {} (global).\n", hook_status);
    println!("  Hook:      {}", hook_path.display());
    println!("  TOK.md:    {} (10 lines)", tok_md_path.display());
    if let Some(path) = &opencode_plugin_path {
        println!("  OpenCode:  {}", path.display());
    }
    println!("  CLAUDE.md: @TOK.md reference added");

    if migrated {
        println!("\n  [ok] Trimmed a 137-line TOK wall in CLAUDE.md");
        println!("              down to @TOK.md (10 lines). Chef’s kiss.");
    }

    // 5. Patch settings.json
    let patch_result =
        patch_settings_json(&hook_path, patch_mode, verbose, install_opencode, graph)?;

    // Report result
    match patch_result {
        PatchResult::Patched => {
            // Already printed by patch_settings_json
        }
        PatchResult::AlreadyPresent => {
            println!("\n  settings.json: hook already present");
            if install_opencode {
                println!("  Restart Claude Code + OpenCode, then poke `git status` to feel it.");
            } else {
                println!("  Restart Claude Code, then run `git status` for a quick vibe check.");
            }
        }
        PatchResult::Declined | PatchResult::Skipped => {
            // Manual instructions already printed by patch_settings_json
        }
    }

    // 6. Generate user-global filters template (~/.config/tok/filters.toml)
    generate_global_filters_template(verbose)?;

    println!(); // Final newline

    Ok(())
}

/// Generate .tok/filters.toml template in the current directory if not present.
fn generate_project_filters_template(verbose: u8) -> Result<()> {
    let tok_dir = std::path::Path::new(".tok");
    let path = tok_dir.join("filters.toml");

    if path.exists() {
        if verbose > 0 {
            eprintln!(".tok/filters.toml already exists, skipping template");
        }
        return Ok(());
    }

    fs::create_dir_all(tok_dir)
        .with_context(|| format!("Failed to create directory: {}", tok_dir.display()))?;
    fs::write(&path, FILTERS_TEMPLATE)
        .with_context(|| format!("Failed to write {}", path.display()))?;

    println!(
        "  filters:   {} (template, edit to add project filters)",
        path.display()
    );
    Ok(())
}

/// Generate ~/.config/tok/filters.toml template if not present.
fn generate_global_filters_template(verbose: u8) -> Result<()> {
    let config_dir = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from(".config"));
    let tok_dir = config_dir.join(crate::core::constants::TOK_DATA_DIR);
    let path = tok_dir.join("filters.toml");

    if path.exists() {
        if verbose > 0 {
            eprintln!("{} already exists, skipping template", path.display());
        }
        return Ok(());
    }

    fs::create_dir_all(&tok_dir)
        .with_context(|| format!("Failed to create directory: {}", tok_dir.display()))?;
    fs::write(&path, FILTERS_GLOBAL_TEMPLATE)
        .with_context(|| format!("Failed to write {}", path.display()))?;

    println!(
        "  filters:   {} (template, edit to add user-global filters)",
        path.display()
    );
    Ok(())
}

/// Hook-only mode: just the hook, no TOK.md
#[cfg(not(unix))]
fn run_hook_only_mode(
    _global: bool,
    _patch_mode: PatchMode,
    _verbose: u8,
    _install_opencode: bool,
    _graph: bool,
) -> Result<()> {
    anyhow::bail!("Hook install requires Unix (macOS/Linux). Use WSL or --claude-md mode.")
}

#[cfg(unix)]
fn run_hook_only_mode(
    global: bool,
    patch_mode: PatchMode,
    verbose: u8,
    install_opencode: bool,
    graph: bool,
) -> Result<()> {
    if !global {
        eprintln!("[warn] Warning: --hook-only only makes sense with --global");
        eprintln!("    For local projects, use default mode or --claude-md");
        return Ok(());
    }

    // Prepare and install hook
    let (_hook_dir, hook_path) = prepare_hook_paths()?;
    let hook_changed = ensure_hook_installed(&hook_path, verbose)?;

    let opencode_plugin_path = if install_opencode {
        let path = prepare_opencode_plugin_path()?;
        ensure_opencode_plugin_installed(&path, verbose)?;
        Some(path)
    } else {
        None
    };

    let hook_status = if hook_changed {
        "installed/updated"
    } else {
        "already up to date"
    };
    println!("\nTOK hook {} (hook-only — lean and mean).\n", hook_status);
    println!("  Hook: {}", hook_path.display());
    if let Some(path) = &opencode_plugin_path {
        println!("  OpenCode: {}", path.display());
    }
    println!(
        "  Note: no TOK.md here — your agent won’t auto-learn meta commands like gain / discover / proxy."
    );

    // Patch settings.json
    let patch_result =
        patch_settings_json(&hook_path, patch_mode, verbose, install_opencode, graph)?;

    // Report result
    match patch_result {
        PatchResult::Patched => {
            // Already printed by patch_settings_json
        }
        PatchResult::AlreadyPresent => {
            println!("\n  settings.json: hook already present");
            if install_opencode {
                println!("  Restart Claude Code + OpenCode, then poke `git status` to feel it.");
            } else {
                println!("  Restart Claude Code, then run `git status` for a quick vibe check.");
            }
        }
        PatchResult::Declined | PatchResult::Skipped => {
            // Manual instructions already printed by patch_settings_json
        }
    }

    println!(); // Final newline

    Ok(())
}

/// Legacy mode: full 137-line injection into CLAUDE.md
fn run_claude_md_mode(global: bool, verbose: u8, install_opencode: bool) -> Result<()> {
    let path = if global {
        resolve_claude_dir()?.join(CLAUDE_MD)
    } else {
        PathBuf::from(CLAUDE_MD)
    };

    if global {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
    }

    if verbose > 0 {
        eprintln!("Writing tok instructions to: {}", path.display());
    }

    if path.exists() {
        let existing = fs::read_to_string(&path)?;
        // upsert_tok_block handles all 4 cases: add, update, unchanged, malformed
        let (new_content, action) = upsert_tok_block(&existing, TOK_INSTRUCTIONS);

        match action {
            TokBlockUpsert::Added => {
                fs::write(&path, new_content)?;
                println!("[ok] Added tok instructions to existing {}", path.display());
            }
            TokBlockUpsert::Updated => {
                fs::write(&path, new_content)?;
                println!("[ok] Updated tok instructions in {}", path.display());
            }
            TokBlockUpsert::Unchanged => {
                println!(
                    "[ok] {} already contains up-to-date tok instructions",
                    path.display()
                );
                return Ok(());
            }
            TokBlockUpsert::Malformed => {
                eprintln!(
                    "[warn] Warning: Found '<!-- tok-instructions' without closing marker in {}",
                    path.display()
                );

                if let Some((line_num, _)) = existing
                    .lines()
                    .enumerate()
                    .find(|(_, line)| line.contains("<!-- tok-instructions"))
                {
                    eprintln!("    Location: line {}", line_num + 1);
                }

                eprintln!("    Action: Manually remove the incomplete block, then re-run:");
                if global {
                    eprintln!("            tok init -g --claude-md");
                } else {
                    eprintln!("            tok init --claude-md");
                }
                return Ok(());
            }
        }
    } else {
        fs::write(&path, TOK_INSTRUCTIONS)?;
        println!("[ok] Created {} with tok instructions", path.display());
    }

    if global {
        if install_opencode {
            let opencode_plugin_path = prepare_opencode_plugin_path()?;
            ensure_opencode_plugin_installed(&opencode_plugin_path, verbose)?;
            println!(
                "[ok] OpenCode plugin installed: {}",
                opencode_plugin_path.display()
            );
        }
        println!("   Claude Code will now use tok in all sessions");
    } else {
        println!("   Claude Code will use tok in this project");
    }

    Ok(())
}

// ─── Windsurf support ─────────────────────────────────────────

/// Embedded Windsurf TOK rules
const WINDSURF_RULES: &str = include_str!("../../../hooks/windsurf/rules.md");

/// Embedded Cline TOK rules
const CLINE_RULES: &str = include_str!("../../../hooks/cline/rules.md");

// ─── Cline / Roo Code support ─────────────────────────────────

fn run_cline_mode(verbose: u8) -> Result<()> {
    // Cline reads .clinerules from the project root (workspace-scoped)
    let rules_path = PathBuf::from(".clinerules");

    let existing = fs::read_to_string(&rules_path).unwrap_or_default();
    if existing.contains("TOK") || existing.contains("tok") {
        println!("\nTOK already configured for Cline in this project.\n");
        println!("  Rules: .clinerules (already present)");
    } else {
        let new_content = if existing.trim().is_empty() {
            CLINE_RULES.to_string()
        } else {
            format!("{}\n\n{}", existing.trim(), CLINE_RULES)
        };
        fs::write(&rules_path, &new_content).context("Failed to write .clinerules")?;

        if verbose > 0 {
            eprintln!("Wrote .clinerules");
        }

        println!("\nTOK configured for Cline.\n");
        println!("  Rules: .clinerules (installed)");
    }
    println!("  Cline will now use tok commands for token savings.");
    println!("  Run `git status` when you’re back in — quick sanity check.\n");

    Ok(())
}

fn run_windsurf_mode(verbose: u8) -> Result<()> {
    // Windsurf reads .windsurfrules from the project root (workspace-scoped).
    // Global rules (~/.codeium/windsurf/memories/global_rules.md) are unreliable.
    let rules_path = PathBuf::from(".windsurfrules");

    let existing = fs::read_to_string(&rules_path).unwrap_or_default();
    if existing.contains("TOK") || existing.contains("tok") {
        println!("\nTOK already configured for Windsurf in this project.\n");
        println!("  Rules: .windsurfrules (already present)");
    } else {
        let new_content = if existing.trim().is_empty() {
            WINDSURF_RULES.to_string()
        } else {
            format!("{}\n\n{}", existing.trim(), WINDSURF_RULES)
        };
        fs::write(&rules_path, &new_content).context("Failed to write .windsurfrules")?;

        if verbose > 0 {
            eprintln!("Wrote .windsurfrules");
        }

        println!("\nTOK configured for Windsurf Cascade.\n");
        println!("  Rules: .windsurfrules (installed)");
    }
    println!("  Cascade will now use tok commands for token savings.");
    println!("  Restart Windsurf, then `git status` to confirm the hook’s alive.\n");

    Ok(())
}

fn run_codex_mode(global: bool, verbose: u8) -> Result<()> {
    let (agents_md_path, tok_md_path) = if global {
        let codex_dir = resolve_codex_dir()?;
        (codex_dir.join(AGENTS_MD), codex_dir.join(TOK_MD))
    } else {
        (PathBuf::from(AGENTS_MD), PathBuf::from(TOK_MD))
    };

    if global {
        if let Some(parent) = agents_md_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "Failed to create Codex config directory: {}",
                    parent.display()
                )
            })?;
        }
    }

    // ISSUE #892: In global mode, use absolute path so @TOK.md resolves
    // from any CWD (worktrees, nested projects). Codex resolves @ references
    // relative to CWD, not the AGENTS.md file location.
    let tok_md_ref = if global {
        format!("@{}", tok_md_path.display())
    } else {
        TOK_MD_REF.to_string()
    };

    write_if_changed(&tok_md_path, TOK_SLIM_CODEX, TOK_MD, verbose)?;
    let added_ref = patch_agents_md(&agents_md_path, &tok_md_ref, verbose)?;

    println!("\nTOK configured for Codex CLI.\n");
    println!("  TOK.md:    {}", tok_md_path.display());
    if added_ref {
        println!("  AGENTS.md: {} reference added", tok_md_ref);
    } else {
        println!("  AGENTS.md: {} reference already present", tok_md_ref);
    }
    if global {
        println!(
            "\n  Codex global instructions path: {}",
            agents_md_path.display()
        );
    } else {
        println!(
            "\n  Codex project instructions path: {}",
            agents_md_path.display()
        );
    }

    Ok(())
}

// --- upsert_tok_block: idempotent TOK block management ---

#[derive(Debug, Clone, Copy, PartialEq)]
enum TokBlockUpsert {
    /// No existing block found — appended new block
    Added,
    /// Existing block found with different content — replaced
    Updated,
    /// Existing block found with identical content — no-op
    Unchanged,
    /// Opening marker found without closing marker — not safe to rewrite
    Malformed,
}

/// Insert or replace the TOK instructions block in `content`.
///
/// Returns `(new_content, action)` describing what happened.
/// The caller decides whether to write `new_content` based on `action`.
fn upsert_tok_block(content: &str, block: &str) -> (String, TokBlockUpsert) {
    let start_marker = "<!-- tok-instructions";
    let end_marker = "<!-- /tok-instructions -->";

    if let Some(start) = content.find(start_marker) {
        if let Some(relative_end) = content[start..].find(end_marker) {
            let end = start + relative_end;
            let end_pos = end + end_marker.len();
            let current_block = content[start..end_pos].trim();
            let desired_block = block.trim();

            if current_block == desired_block {
                return (content.to_string(), TokBlockUpsert::Unchanged);
            }

            // Replace stale block with desired block
            let before = content[..start].trim_end();
            let after = content[end_pos..].trim_start();

            let result = match (before.is_empty(), after.is_empty()) {
                (true, true) => desired_block.to_string(),
                (true, false) => format!("{desired_block}\n\n{after}"),
                (false, true) => format!("{before}\n\n{desired_block}"),
                (false, false) => format!("{before}\n\n{desired_block}\n\n{after}"),
            };

            return (result, TokBlockUpsert::Updated);
        }

        // Opening marker without closing marker — malformed
        return (content.to_string(), TokBlockUpsert::Malformed);
    }

    // No existing block — append
    let trimmed = content.trim();
    if trimmed.is_empty() {
        (block.to_string(), TokBlockUpsert::Added)
    } else {
        (
            format!("{trimmed}\n\n{}", block.trim()),
            TokBlockUpsert::Added,
        )
    }
}

/// Patch CLAUDE.md: add @TOK.md, migrate if old block exists
fn patch_claude_md(path: &Path, verbose: u8) -> Result<bool> {
    let mut content = if path.exists() {
        fs::read_to_string(path)?
    } else {
        String::new()
    };

    let mut migrated = false;

    // Check for old block and migrate
    if content.contains("<!-- tok-instructions") {
        let (new_content, did_migrate) = remove_tok_block(&content);
        if did_migrate {
            content = new_content;
            migrated = true;
            if verbose > 0 {
                eprintln!("Migrated: removed old TOK block from CLAUDE.md");
            }
        }
    }

    // Check if @TOK.md already present
    if content.contains(TOK_MD_REF) {
        if verbose > 0 {
            eprintln!("@TOK.md reference already present in CLAUDE.md");
        }
        if migrated {
            fs::write(path, content)?;
        }
        return Ok(migrated);
    }

    // Add @TOK.md
    let new_content = if content.is_empty() {
        "@TOK.md\n".to_string()
    } else {
        format!("{}\n\n@TOK.md\n", content.trim())
    };

    fs::write(path, new_content)?;

    if verbose > 0 {
        eprintln!("Added @TOK.md reference to CLAUDE.md");
    }

    Ok(migrated)
}

/// Patch AGENTS.md: add @TOK.md (or absolute path), migrate old inline block if present
fn patch_agents_md(path: &Path, tok_md_ref: &str, verbose: u8) -> Result<bool> {
    let mut content = if path.exists() {
        fs::read_to_string(path)
            .with_context(|| format!("Failed to read AGENTS.md: {}", path.display()))?
    } else {
        String::new()
    };

    let mut migrated = false;
    if content.contains("<!-- tok-instructions") {
        let (new_content, did_migrate) = remove_tok_block(&content);
        if did_migrate {
            content = new_content;
            migrated = true;
            if verbose > 0 {
                eprintln!("Migrated: removed old TOK block from AGENTS.md");
            }
        }
    }

    // ISSUE #892: Check for both relative and absolute @TOK.md references
    if content.contains(TOK_MD_REF) || content.contains(tok_md_ref) {
        if verbose > 0 {
            eprintln!("{} reference already present in AGENTS.md", tok_md_ref);
        }
        // ISSUE #892: Migrate old relative @TOK.md to absolute path if needed
        if tok_md_ref != TOK_MD_REF && content.contains(TOK_MD_REF) && !content.contains(tok_md_ref)
        {
            content = content.replace(TOK_MD_REF, tok_md_ref);
            atomic_write(path, &content)
                .with_context(|| format!("Failed to write AGENTS.md: {}", path.display()))?;
            if verbose > 0 {
                eprintln!("Migrated {} to {}", TOK_MD_REF, tok_md_ref);
            }
            return Ok(true);
        }
        if migrated {
            atomic_write(path, &content)
                .with_context(|| format!("Failed to write AGENTS.md: {}", path.display()))?;
        }
        return Ok(false);
    }

    let new_content = if content.is_empty() {
        format!("{}\n", tok_md_ref)
    } else {
        format!("{}\n\n{}\n", content.trim(), tok_md_ref)
    };

    atomic_write(path, &new_content)
        .with_context(|| format!("Failed to write AGENTS.md: {}", path.display()))?;
    if verbose > 0 {
        eprintln!("Added {} reference to AGENTS.md", tok_md_ref);
    }

    Ok(true)
}

fn remove_tok_reference_from_agents(path: &Path, verbose: u8) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }

    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read AGENTS.md: {}", path.display()))?;
    if !content.contains(TOK_MD_REF) {
        return Ok(false);
    }

    let new_content = content
        .lines()
        .filter(|line| !line.trim().starts_with(TOK_MD_REF))
        .collect::<Vec<_>>()
        .join("\n");
    let cleaned = clean_double_blanks(&new_content);
    atomic_write(path, &cleaned)
        .with_context(|| format!("Failed to write AGENTS.md: {}", path.display()))?;

    if verbose > 0 {
        eprintln!(
            "Removed @TOK.md reference from AGENTS.md: {}",
            path.display()
        );
    }

    Ok(true)
}

/// Remove old TOK block from CLAUDE.md (migration helper)
fn remove_tok_block(content: &str) -> (String, bool) {
    if let (Some(start), Some(end)) = (
        content.find("<!-- tok-instructions"),
        content.find("<!-- /tok-instructions -->"),
    ) {
        let end_pos = end + "<!-- /tok-instructions -->".len();
        let before = content[..start].trim_end();
        let after = content[end_pos..].trim_start();

        let result = if after.is_empty() {
            before.to_string()
        } else {
            format!("{}\n\n{}", before, after)
        };

        (result, true) // migrated
    } else if content.contains("<!-- tok-instructions") {
        eprintln!("[warn] Warning: Found '<!-- tok-instructions' without closing marker.");
        eprintln!("    This can happen if CLAUDE.md was manually edited.");

        // Find line number
        if let Some((line_num, _)) = content
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains("<!-- tok-instructions"))
        {
            eprintln!("    Location: line {}", line_num + 1);
        }

        eprintln!("    Action: Manually remove the incomplete block, then re-run:");
        eprintln!("            tok init -g");
        (content.to_string(), false)
    } else {
        (content.to_string(), false)
    }
}

/// Prepare OpenCode plugin directory and return install path
fn prepare_opencode_plugin_path() -> Result<PathBuf> {
    let path = user_opencode_plugin_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create OpenCode plugin directory: {}",
                parent.display()
            )
        })?;
    }
    Ok(path)
}

/// Write OpenCode plugin file if missing or outdated
fn ensure_opencode_plugin_installed(path: &Path, verbose: u8) -> Result<bool> {
    write_if_changed(path, OPENCODE_PLUGIN, "OpenCode plugin", verbose)
}

/// Remove OpenCode plugin file
fn remove_opencode_plugin(verbose: u8) -> Result<Vec<PathBuf>> {
    let path = user_opencode_plugin_path()?;
    let mut removed = Vec::new();

    if path.exists() {
        fs::remove_file(&path)
            .with_context(|| format!("Failed to remove OpenCode plugin: {}", path.display()))?;
        if verbose > 0 {
            eprintln!("Removed OpenCode plugin: {}", path.display());
        }
        removed.push(path);
    }

    Ok(removed)
}

// ─── Cursor Agent support ─────────────────────────────────────────────

/// Install Cursor hooks: hook script + hooks.json
fn install_cursor_hooks(verbose: u8, graph: bool) -> Result<()> {
    let cursor_dir = resolve_cursor_dir()?;
    let hooks_dir = cursor_dir.join("hooks");
    fs::create_dir_all(&hooks_dir).with_context(|| {
        format!(
            "Failed to create Cursor hooks directory: {}",
            hooks_dir.display()
        )
    })?;

    // 1. Write rewrite hook script
    let hook_path = hooks_dir.join(REWRITE_HOOK_FILE);
    let hook_changed = write_if_changed(&hook_path, CURSOR_REWRITE_HOOK, "Cursor hook", verbose)?;

    // 2. Write session awareness hook script
    let session_path = hooks_dir.join(SESSION_HOOK_FILE);
    let session_changed = write_if_changed(
        &session_path,
        CURSOR_SESSION_HOOK,
        "Cursor session hook",
        verbose,
    )?;

    install_memory_hook_scripts(&hooks_dir, "cursor", verbose)?;
    if graph {
        install_graph_hook_scripts(&hooks_dir, "cursor", verbose)?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::Permissions::from_mode(0o755);
        fs::set_permissions(&hook_path, mode.clone()).with_context(|| {
            format!(
                "Failed to set Cursor hook permissions: {}",
                hook_path.display()
            )
        })?;
        fs::set_permissions(&session_path, mode).with_context(|| {
            format!(
                "Failed to set Cursor session hook permissions: {}",
                session_path.display()
            )
        })?;
    }

    // 3. Create or patch hooks.json (preToolUse + sessionStart + memory lifecycle)
    let hooks_json_path = cursor_dir.join(HOOKS_JSON);
    let patched = patch_cursor_hooks_json(&hooks_json_path, verbose, graph)?;

    // Report
    let any_changed = hook_changed || session_changed;
    let hook_status = if any_changed {
        "installed/updated"
    } else {
        "already up to date"
    };
    println!("\nCursor hook {} (global).\n", hook_status);
    println!("  Hook:       {}", hook_path.display());
    println!("  Session:    {}", session_path.display());
    println!("  hooks.json: {}", hooks_json_path.display());

    if patched {
        println!("  hooks.json: TOK entries added (preToolUse + sessionStart)");
    } else {
        println!("  hooks.json: TOK entries already present");
    }

    println!("  Cursor reloads hooks.json on its own — run `git status` to verify.\n");

    Ok(())
}

/// Patch ~/.cursor/hooks.json to add TOK preToolUse + sessionStart hooks.
/// Returns true if the file was modified.
fn patch_cursor_hooks_json(path: &Path, verbose: u8, graph: bool) -> Result<bool> {
    let mut root = if path.exists() {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        if content.trim().is_empty() {
            serde_json::json!({ "version": 1 })
        } else {
            serde_json::from_str(&content)
                .with_context(|| format!("Failed to parse {} as JSON", path.display()))?
        }
    } else {
        serde_json::json!({ "version": 1 })
    };

    let has_rewrite = cursor_hook_entry_present(&root, "preToolUse", REWRITE_HOOK_FILE);
    let has_session = cursor_hook_entry_present(&root, "sessionStart", SESSION_HOOK_FILE);
    let has_cache = cursor_hook_entry_present(&root, "beforeSubmitPrompt", MEMORY_HOOK_MARKER);
    let has_extract = cursor_hook_entry_present(&root, "afterAgentResponse", MEMORY_HOOK_MARKER);
    // Cursor takes one sessionStart hook and tok-session.sh already composes
    // the TOK awareness text with memory, so the graph joins there rather than
    // as a second entry that would be ignored.
    let wants_postedit =
        graph && !cursor_hook_entry_present(&root, "afterFileEdit", GRAPH_HOOK_MARKER);

    if has_rewrite && has_session && has_cache && has_extract && !wants_postedit {
        if verbose > 0 {
            eprintln!("Cursor hooks.json: all TOK entries already present");
        }
        return Ok(false);
    }

    if !has_rewrite {
        insert_cursor_hook_array_entry(
            &mut root,
            "preToolUse",
            serde_json::json!({
                "command": format!("./hooks/{}", REWRITE_HOOK_FILE),
                "matcher": "Shell"
            }),
        );
    }
    if !has_session {
        insert_cursor_hook_array_entry(
            &mut root,
            "sessionStart",
            serde_json::json!({
                "command": format!("./hooks/{}", SESSION_HOOK_FILE)
            }),
        );
    }
    if !has_cache {
        insert_cursor_hook_array_entry(
            &mut root,
            "beforeSubmitPrompt",
            serde_json::json!({
                "command": format!("./hooks/{}", MEMORY_CACHE_PROMPT_HOOK_FILE)
            }),
        );
    }
    if !has_extract {
        insert_cursor_hook_array_entry(
            &mut root,
            "afterAgentResponse",
            serde_json::json!({
                "command": format!("./hooks/{}", MEMORY_EXTRACT_HOOK_FILE)
            }),
        );
    }
    if wants_postedit {
        insert_cursor_hook_array_entry(
            &mut root,
            "afterFileEdit",
            serde_json::json!({
                "command": format!("./hooks/{}", GRAPH_POSTEDIT_HOOK_FILE)
            }),
        );
    }

    // Backup if exists
    if path.exists() {
        let backup_path = path.with_extension("json.bak");
        fs::copy(path, &backup_path)
            .with_context(|| format!("Failed to backup to {}", backup_path.display()))?;
        if verbose > 0 {
            eprintln!("Backup: {}", backup_path.display());
        }
    }

    // Atomic write
    let serialized =
        serde_json::to_string_pretty(&root).context("Failed to serialize hooks.json")?;
    atomic_write(path, &serialized)?;

    Ok(true)
}

/// Check if a TOK entry exists in a specific Cursor hooks.json event array.
fn cursor_hook_entry_present(root: &serde_json::Value, event: &str, file_name: &str) -> bool {
    root.get("hooks")
        .and_then(|h| h.get(event))
        .and_then(|p| p.as_array())
        .is_some_and(|arr| {
            arr.iter().any(|entry| {
                entry
                    .get("command")
                    .and_then(|c| c.as_str())
                    .is_some_and(|cmd| cmd.contains(file_name))
            })
        })
}

/// Insert an entry into a Cursor hooks.json event array.
fn insert_cursor_hook_array_entry(
    root: &mut serde_json::Value,
    event: &str,
    entry: serde_json::Value,
) {
    let root_obj = match root.as_object_mut() {
        Some(obj) => obj,
        None => {
            *root = serde_json::json!({ "version": 1 });
            root.as_object_mut()
                .expect("Just created object, must succeed")
        }
    };

    root_obj.entry("version").or_insert(serde_json::json!(1));

    let hooks = root_obj
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .expect("hooks must be an object");

    let arr = hooks
        .entry(event)
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .expect("hook event array must be an array");

    arr.push(entry);
}

/// Remove Cursor TOK artifacts: hook scripts + hooks.json entries
fn remove_cursor_hooks(verbose: u8) -> Result<Vec<String>> {
    let cursor_dir = resolve_cursor_dir()?;
    let mut removed = Vec::new();

    // 1. Remove hook scripts
    for file_name in [
        REWRITE_HOOK_FILE,
        SESSION_HOOK_FILE,
        MEMORY_SESSION_HOOK_FILE,
        MEMORY_PROMPT_HOOK_FILE,
        MEMORY_EXTRACT_HOOK_FILE,
        MEMORY_CACHE_PROMPT_HOOK_FILE,
        GRAPH_SESSION_HOOK_FILE,
        GRAPH_POSTEDIT_HOOK_FILE,
    ] {
        let path = cursor_dir.join(HOOKS_SUBDIR).join(file_name);
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("Failed to remove Cursor hook: {}", path.display()))?;
            removed.push(format!("Cursor hook: {}", path.display()));
        }
    }

    // 2. Remove TOK entries from hooks.json
    let hooks_json_path = cursor_dir.join(HOOKS_JSON);
    if hooks_json_path.exists() {
        let content = fs::read_to_string(&hooks_json_path)
            .with_context(|| format!("Failed to read {}", hooks_json_path.display()))?;

        if !content.trim().is_empty() {
            if let Ok(mut root) = serde_json::from_str::<serde_json::Value>(&content) {
                let removed_any = remove_cursor_hooks_from_json(&mut root);
                if removed_any {
                    let backup_path = hooks_json_path.with_extension("json.bak");
                    fs::copy(&hooks_json_path, &backup_path).ok();

                    let serialized = serde_json::to_string_pretty(&root)
                        .context("Failed to serialize hooks.json")?;
                    atomic_write(&hooks_json_path, &serialized)?;

                    removed.push("Cursor hooks.json: removed TOK entries".to_string());

                    if verbose > 0 {
                        eprintln!("Removed TOK hooks from Cursor hooks.json");
                    }
                }
            }
        }
    }

    Ok(removed)
}

/// Remove all TOK entries from Cursor hooks.json (preToolUse + sessionStart).
/// Returns true if any entry was found and removed.
fn remove_cursor_hooks_from_json(root: &mut serde_json::Value) -> bool {
    let tok_files = [
        REWRITE_HOOK_FILE,
        SESSION_HOOK_FILE,
        MEMORY_SESSION_HOOK_FILE,
        MEMORY_PROMPT_HOOK_FILE,
        MEMORY_EXTRACT_HOOK_FILE,
        MEMORY_CACHE_PROMPT_HOOK_FILE,
        GRAPH_SESSION_HOOK_FILE,
        GRAPH_POSTEDIT_HOOK_FILE,
    ];
    let events = [
        "preToolUse",
        "sessionStart",
        "beforeSubmitPrompt",
        "afterAgentResponse",
        "afterFileEdit",
        "stop",
    ];
    let mut any_removed = false;

    for event in events {
        let arr = match root
            .get_mut("hooks")
            .and_then(|h| h.get_mut(event))
            .and_then(|p| p.as_array_mut())
        {
            Some(a) => a,
            None => continue,
        };

        let before = arr.len();
        arr.retain(|entry| {
            !entry
                .get("command")
                .and_then(|c| c.as_str())
                .is_some_and(|cmd| tok_files.iter().any(|f| cmd.contains(f)))
        });
        if arr.len() < before {
            any_removed = true;
        }
    }

    any_removed
}

/// Show current tok configuration
pub fn show_config(codex: bool) -> Result<()> {
    if codex {
        return show_codex_config();
    }

    show_claude_config()
}

fn show_claude_config() -> Result<()> {
    let claude_dir = resolve_claude_dir()?;
    let hook_path = claude_dir.join(HOOKS_SUBDIR).join(REWRITE_HOOK_FILE);
    let tok_md_path = claude_dir.join(TOK_MD);
    let global_claude_md = claude_dir.join(CLAUDE_MD);
    let local_claude_md = PathBuf::from(CLAUDE_MD);

    println!("tok Configuration:\n");

    // Check hook
    if hook_path.exists() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = fs::metadata(&hook_path)?;
            let perms = metadata.permissions();
            let is_executable = perms.mode() & 0o111 != 0;

            let hook_content = fs::read_to_string(&hook_path)?;
            let has_guards =
                hook_content.contains("command -v tok") && hook_content.contains("command -v jq");
            let is_thin_delegator = hook_content.contains("tok rewrite");
            let hook_version = super::hook_check::parse_hook_version(&hook_content);

            if !is_executable {
                println!(
                    "[warn] Hook: {} (NOT executable - run: chmod +x)",
                    hook_path.display()
                );
            } else if !is_thin_delegator {
                println!(
                    "[warn] Hook: {} (outdated — inline logic, not thin delegator)",
                    hook_path.display()
                );
                println!(
                    "   → Run `tok init --global` to upgrade to the single source of truth hook"
                );
            } else if is_executable && has_guards {
                println!(
                    "[ok] Hook: {} (thin delegator, version {})",
                    hook_path.display(),
                    hook_version
                );
            } else {
                println!(
                    "[warn] Hook: {} (no guards - outdated)",
                    hook_path.display()
                );
            }
        }

        #[cfg(not(unix))]
        {
            println!("[ok] Hook: {} (exists)", hook_path.display());
        }
    } else {
        println!("[--] Hook: not found");
    }

    // Check TOK.md
    if tok_md_path.exists() {
        println!("[ok] TOK.md: {} (slim mode)", tok_md_path.display());
    } else {
        println!("[--] TOK.md: not found");
    }

    // Check hook integrity
    match integrity::verify_hook_at(&hook_path) {
        Ok(integrity::IntegrityStatus::Verified) => {
            println!("[ok] Integrity: hook hash verified");
        }
        Ok(integrity::IntegrityStatus::Tampered { .. }) => {
            println!("[FAIL] Integrity: hook modified outside tok init (run: tok verify)");
        }
        Ok(integrity::IntegrityStatus::NoBaseline) => {
            println!("[warn] Integrity: no baseline hash (run: tok init -g to establish)");
        }
        Ok(integrity::IntegrityStatus::NotInstalled)
        | Ok(integrity::IntegrityStatus::OrphanedHash) => {
            // Don't show integrity line if hook isn't installed
        }
        Err(_) => {
            println!("[warn] Integrity: check failed");
        }
    }

    // Check global CLAUDE.md
    if global_claude_md.exists() {
        let content = fs::read_to_string(&global_claude_md)?;
        if content.contains(TOK_MD_REF) {
            println!("[ok] Global (~/.claude/CLAUDE.md): @TOK.md reference");
        } else if content.contains("<!-- tok-instructions") {
            println!(
                "[warn] Global (~/.claude/CLAUDE.md): old TOK block (run: tok init -g to migrate)"
            );
        } else {
            println!("[--] Global (~/.claude/CLAUDE.md): exists but tok not configured");
        }
    } else {
        println!("[--] Global (~/.claude/CLAUDE.md): not found");
    }

    // Check local CLAUDE.md
    if local_claude_md.exists() {
        let content = fs::read_to_string(&local_claude_md)?;
        if content.contains("tok") {
            println!("[ok] Local (./CLAUDE.md): tok enabled");
        } else {
            println!("[--] Local (./CLAUDE.md): exists but tok not configured");
        }
    } else {
        println!("[--] Local (./CLAUDE.md): not found");
    }

    // Check settings.json
    let settings_path = claude_dir.join(SETTINGS_JSON);
    if settings_path.exists() {
        let content = fs::read_to_string(&settings_path)?;
        if !content.trim().is_empty() {
            if let Ok(root) = serde_json::from_str::<serde_json::Value>(&content) {
                let hook_command = hook_path.display().to_string();
                if hook_already_present(&root, &hook_command) {
                    println!("[ok] settings.json: TOK rewrite hook configured");
                    if claude_hook_command_present(&root, SESSION_START_KEY, MEMORY_HOOK_MARKER) {
                        println!("[ok] settings.json: TOK agent memory hooks configured");
                    } else {
                        println!(
                            "[--] settings.json: agent memory hooks missing (run: tok init -g --auto-patch)"
                        );
                    }
                } else {
                    println!("[warn] settings.json: exists but TOK hook not configured");
                    println!("    Run: tok init -g --auto-patch");
                }
            } else {
                println!("[warn] settings.json: exists but invalid JSON");
            }
        } else {
            println!("[--] settings.json: empty");
        }
    } else {
        println!("[--] settings.json: not found");
    }

    // Check OpenCode plugin
    match user_opencode_plugin_path() {
        Ok(plugin) if plugin.exists() => {
            println!("[ok] OpenCode: plugin installed ({})", plugin.display());
        }
        Ok(_) => println!("[--] OpenCode: plugin not found"),
        Err(_) => println!("[--] OpenCode: config dir not found"),
    }

    // Check Cursor hooks
    if let Ok(cursor_dir) = resolve_cursor_dir() {
        let cursor_hook = cursor_dir.join(HOOKS_SUBDIR).join(REWRITE_HOOK_FILE);
        let cursor_hooks_json = cursor_dir.join(HOOKS_JSON);

        if cursor_hook.exists() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let meta = fs::metadata(&cursor_hook)?;
                let is_executable = meta.permissions().mode() & 0o111 != 0;
                let content = fs::read_to_string(&cursor_hook)?;
                let is_thin = content.contains("tok rewrite");

                if !is_executable {
                    println!(
                        "[warn] Cursor hook: {} (NOT executable - run: chmod +x)",
                        cursor_hook.display()
                    );
                } else if is_thin {
                    println!(
                        "[ok] Cursor hook: {} (thin delegator)",
                        cursor_hook.display()
                    );
                } else {
                    println!(
                        "[warn] Cursor hook: {} (outdated - missing tok rewrite delegation)",
                        cursor_hook.display()
                    );
                }
            }

            #[cfg(not(unix))]
            {
                println!("[ok] Cursor hook: {} (exists)", cursor_hook.display());
            }
        } else {
            println!("[--] Cursor hook: not found");
        }

        let cursor_session = cursor_dir.join(HOOKS_SUBDIR).join(SESSION_HOOK_FILE);
        if cursor_session.exists() {
            println!(
                "[ok] Cursor session hook: {} (awareness injector)",
                cursor_session.display()
            );
        } else {
            println!("[--] Cursor session hook: not found (run: tok init -g --agent cursor)");
        }

        if cursor_hooks_json.exists() {
            let content = fs::read_to_string(&cursor_hooks_json)?;
            if !content.trim().is_empty() {
                if let Ok(root) = serde_json::from_str::<serde_json::Value>(&content) {
                    let has_rewrite =
                        cursor_hook_entry_present(&root, "preToolUse", REWRITE_HOOK_FILE);
                    let has_session =
                        cursor_hook_entry_present(&root, "sessionStart", SESSION_HOOK_FILE);
                    let has_memory = cursor_hook_entry_present(
                        &root,
                        "beforeSubmitPrompt",
                        MEMORY_CACHE_PROMPT_HOOK_FILE,
                    ) && cursor_hook_entry_present(
                        &root,
                        "afterAgentResponse",
                        MEMORY_EXTRACT_HOOK_FILE,
                    );
                    if has_rewrite && has_session && has_memory {
                        println!(
                            "[ok] Cursor hooks.json: TOK configured (rewrite + session + memory)"
                        );
                    } else if has_rewrite && has_session {
                        println!(
                            "[ok] Cursor hooks.json: TOK configured (preToolUse + sessionStart)"
                        );
                        println!(
                            "[--] Cursor hooks.json: memory hooks missing (run: tok init -g --agent cursor)"
                        );
                    } else if has_rewrite {
                        println!("[ok] Cursor hooks.json: TOK preToolUse configured");
                        println!("[--] Cursor hooks.json: sessionStart missing (run: tok init -g --agent cursor)");
                    } else {
                        println!("[warn] Cursor hooks.json: exists but TOK not configured");
                        println!("    Run: tok init -g --agent cursor");
                    }
                } else {
                    println!("[warn] Cursor hooks.json: exists but invalid JSON");
                }
            } else {
                println!("[--] Cursor hooks.json: empty");
            }
        } else {
            println!("[--] Cursor hooks.json: not found");
        }
    } else {
        println!("[--] Cursor: home dir not found");
    }

    println!("\nUsage:");
    println!("  tok init              # Full injection into local CLAUDE.md");
    println!("  tok init -g           # Hook + TOK.md + @TOK.md + settings.json (recommended)");
    println!("  tok init -g --auto-patch    # Same as above but no prompt");
    println!("  tok init -g --no-patch      # Skip settings.json (manual setup)");
    println!("  tok init -g --uninstall     # Remove all TOK artifacts");
    println!("  tok init -g --claude-md     # Legacy: full injection into ~/.claude/CLAUDE.md");
    println!("  tok init -g --hook-only     # Hook only, no TOK.md");
    println!("  tok init --codex            # Configure local AGENTS.md + TOK.md");
    println!("  tok init -g --codex         # Configure ~/.codex/AGENTS.md + ~/.codex/TOK.md");
    println!("  tok init -g --opencode      # OpenCode plugin only");
    println!("  tok init -g --agent cursor  # Install Cursor Agent hooks");

    Ok(())
}

fn show_codex_config() -> Result<()> {
    let codex_dir = resolve_codex_dir()?;
    let global_agents_md = codex_dir.join(AGENTS_MD);
    let global_tok_md = codex_dir.join(TOK_MD);
    let local_agents_md = PathBuf::from(AGENTS_MD);
    let local_tok_md = PathBuf::from(TOK_MD);

    println!("tok Configuration (Codex CLI):\n");

    if global_tok_md.exists() {
        println!("[ok] Global TOK.md: {}", global_tok_md.display());
    } else {
        println!("[--] Global TOK.md: not found");
    }

    if global_agents_md.exists() {
        let content = fs::read_to_string(&global_agents_md)?;
        if content.contains(TOK_MD_REF) {
            println!("[ok] Global AGENTS.md: @TOK.md reference");
        } else if content.contains("<!-- tok-instructions") {
            println!("[!!] Global AGENTS.md: old inline TOK block");
        } else {
            println!("[--] Global AGENTS.md: exists but tok not configured");
        }
    } else {
        println!("[--] Global AGENTS.md: not found");
    }

    if local_tok_md.exists() {
        println!("[ok] Local TOK.md: {}", local_tok_md.display());
    } else {
        println!("[--] Local TOK.md: not found");
    }

    if local_agents_md.exists() {
        let content = fs::read_to_string(&local_agents_md)?;
        if content.contains(TOK_MD_REF) {
            println!("[ok] Local AGENTS.md: @TOK.md reference");
        } else if content.contains("<!-- tok-instructions") {
            println!("[!!] Local AGENTS.md: old inline TOK block");
        } else {
            println!("[--] Local AGENTS.md: exists but tok not configured");
        }
    } else {
        println!("[--] Local AGENTS.md: not found");
    }

    println!("\nUsage:");
    println!("  tok init --codex              # Configure local AGENTS.md + TOK.md");
    println!("  tok init -g --codex           # Configure ~/.codex/AGENTS.md + ~/.codex/TOK.md");
    println!("  tok init -g --codex --uninstall  # Remove global Codex TOK artifacts");

    Ok(())
}

fn run_opencode_only_mode(verbose: u8) -> Result<()> {
    let opencode_plugin_path = prepare_opencode_plugin_path()?;
    ensure_opencode_plugin_installed(&opencode_plugin_path, verbose)?;
    println!("\nOpenCode plugin installed (global).\n");
    println!("  OpenCode: {}", opencode_plugin_path.display());
    println!("  Restart OpenCode, then `git status` for a quick check.\n");
    Ok(())
}

// ─── Gemini CLI support ───────────────────────────────────────────

/// Gemini hook wrapper script — delegates to `tok hook gemini`
const GEMINI_HOOK_SCRIPT: &str = r#"#!/bin/bash
exec tok hook gemini
"#;

/// Entry point for `tok init --gemini`
pub fn run_gemini(global: bool, hook_only: bool, patch_mode: PatchMode, verbose: u8) -> Result<()> {
    if !global {
        anyhow::bail!("Gemini support is global-only. Use: tok init -g --gemini");
    }

    let gemini_dir = resolve_gemini_dir()?;
    fs::create_dir_all(&gemini_dir).with_context(|| {
        format!(
            "Failed to create Gemini config dir: {}",
            gemini_dir.display()
        )
    })?;

    // 1. Install hook script
    let hook_dir = gemini_dir.join("hooks");
    fs::create_dir_all(&hook_dir)
        .with_context(|| format!("Failed to create hook dir: {}", hook_dir.display()))?;
    let hook_path = hook_dir.join(GEMINI_HOOK_FILE);
    write_if_changed(&hook_path, GEMINI_HOOK_SCRIPT, "Gemini hook", verbose)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755))
            .with_context(|| format!("Failed to set hook permissions: {}", hook_path.display()))?;
    }

    // 2. Install GEMINI.md (TOK awareness for Gemini)
    if !hook_only {
        let gemini_md_path = gemini_dir.join(GEMINI_MD);
        // Reuse the same slim TOK awareness content
        write_if_changed(&gemini_md_path, TOK_SLIM, GEMINI_MD, verbose)?;
    }

    // 3. Patch ~/.gemini/settings.json
    patch_gemini_settings(&gemini_dir, &hook_path, patch_mode, verbose)?;

    println!("\nGemini CLI hook installed (global).\n");
    println!("  Hook: {}", hook_path.display());
    if !hook_only {
        println!("  GEMINI.md: {}", gemini_dir.join(GEMINI_MD).display());
    }
    println!("  Restart Gemini CLI, then `git status` to see the squeeze.\n");
    Ok(())
}

/// Patch ~/.gemini/settings.json with the BeforeTool hook
fn patch_gemini_settings(
    gemini_dir: &Path,
    hook_path: &Path,
    patch_mode: PatchMode,
    verbose: u8,
) -> Result<()> {
    let settings_path = gemini_dir.join(SETTINGS_JSON);
    let hook_cmd = hook_path.to_string_lossy().to_string();

    // Read or create settings.json
    let mut settings: serde_json::Value = if settings_path.exists() {
        let content = fs::read_to_string(&settings_path)
            .with_context(|| format!("Failed to read {}", settings_path.display()))?;
        serde_json::from_str(&content).unwrap_or(serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    let hook_dir = gemini_dir.join(HOOKS_SUBDIR);
    install_memory_hook_scripts(&hook_dir, "gemini", verbose)?;
    let memory_changed = patch_gemini_memory_hooks(&mut settings, &hook_dir)?;

    let before_tool_pointer = format!("/hooks/{}", BEFORE_TOOL_KEY);
    let rewrite_present = settings.pointer(&before_tool_pointer).is_some_and(|hooks| {
        hooks.as_array().is_some_and(|arr| {
            arr.iter().any(|h| {
                h.pointer("/hooks/0/command")
                    .and_then(|v| v.as_str())
                    .is_some_and(|c| c.contains("tok"))
            })
        })
    });
    if rewrite_present {
        if memory_changed {
            let content = serde_json::to_string_pretty(&settings)?;
            let tmp = NamedTempFile::new_in(gemini_dir)?;
            fs::write(tmp.path(), &content)?;
            tmp.persist(&settings_path)
                .with_context(|| format!("Failed to write {}", settings_path.display()))?;
            if verbose > 0 {
                eprintln!("Patched {} (agent memory hooks)", settings_path.display());
            }
        } else if verbose > 0 {
            eprintln!("Gemini settings.json already has TOK hooks");
        }
        return Ok(());
    }

    // Ask user before patching
    if patch_mode == PatchMode::Skip {
        println!(
            "\nManual setup needed: add TOK hook to {}\n\
             See: https://github.com/MantisWare/tok#gemini-cli",
            settings_path.display()
        );
        return Ok(());
    }

    if patch_mode == PatchMode::Ask {
        print!("Patch {} with TOK hook? [y/N] ", settings_path.display());
        std::io::Write::flush(&mut std::io::stdout())?;
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        if !answer.trim().eq_ignore_ascii_case("y") {
            println!("Skipped. Add hook manually later.");
            return Ok(());
        }
    }

    // Build hook entry matching Gemini CLI format
    let hook_entry = serde_json::json!({
        "matcher": "run_shell_command",
        "hooks": [{
            "type": "command",
            "command": hook_cmd
        }]
    });

    // Insert into settings
    let hooks = settings
        .as_object_mut()
        .context("settings.json is not an object")?
        .entry("hooks")
        .or_insert(serde_json::json!({}));

    let before_tool = hooks
        .as_object_mut()
        .context("hooks is not an object")?
        .entry(BEFORE_TOOL_KEY)
        .or_insert(serde_json::json!([]));

    before_tool
        .as_array_mut()
        .context("BeforeTool is not an array")?
        .push(hook_entry);

    // Write atomically
    let content = serde_json::to_string_pretty(&settings)?;
    let tmp = NamedTempFile::new_in(gemini_dir)?;
    fs::write(tmp.path(), &content)?;
    tmp.persist(&settings_path)
        .with_context(|| format!("Failed to write {}", settings_path.display()))?;

    if verbose > 0 {
        eprintln!("Patched {}", settings_path.display());
    }

    Ok(())
}

/// Remove Gemini artifacts during uninstall
fn uninstall_gemini(verbose: u8) -> Result<Vec<String>> {
    let mut removed = Vec::new();
    let gemini_dir = match resolve_gemini_dir() {
        Ok(d) => d,
        Err(_) => return Ok(removed),
    };

    // Remove hooks
    let hook_dir = gemini_dir.join(HOOKS_SUBDIR);
    for name in [
        GEMINI_HOOK_FILE,
        MEMORY_SESSION_HOOK_FILE,
        MEMORY_PROMPT_HOOK_FILE,
        MEMORY_EXTRACT_HOOK_FILE,
    ] {
        let path = hook_dir.join(name);
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("Failed to remove {}", path.display()))?;
            removed.push(format!("Gemini hook: {}", path.display()));
        }
    }

    // Remove GEMINI.md
    let gemini_md = gemini_dir.join(GEMINI_MD);
    if gemini_md.exists() {
        fs::remove_file(&gemini_md)
            .with_context(|| format!("Failed to remove {}", gemini_md.display()))?;
        removed.push(format!("GEMINI.md: {}", gemini_md.display()));
    }

    // Remove hook from settings.json
    let settings_path = gemini_dir.join(SETTINGS_JSON);
    if settings_path.exists() {
        let content = fs::read_to_string(&settings_path)?;
        if let Ok(mut settings) = serde_json::from_str::<serde_json::Value>(&content) {
            let mut changed = false;
            let bt_pointer = format!("/hooks/{}", BEFORE_TOOL_KEY);
            if let Some(arr) = settings
                .pointer_mut(&bt_pointer)
                .and_then(|v| v.as_array_mut())
            {
                let before = arr.len();
                arr.retain(|h| {
                    !h.pointer("/hooks/0/command")
                        .and_then(|v| v.as_str())
                        .is_some_and(|c| c.contains("tok"))
                });
                if arr.len() < before {
                    changed = true;
                    removed.push("Gemini settings.json: removed TOK hook entry".to_string());
                }
            }
            if remove_memory_hooks_from_gemini_json(&mut settings) {
                changed = true;
                removed.push("Gemini settings.json: removed TOK memory hooks".to_string());
            }
            if changed {
                let new_content = serde_json::to_string_pretty(&settings)?;
                fs::write(&settings_path, new_content)?;
            }
        }
    }

    if verbose > 0 && !removed.is_empty() {
        eprintln!("Gemini artifacts removed");
    }

    Ok(removed)
}

// ── Copilot integration ─────────────────────────────────────

const COPILOT_HOOK_JSON: &str = r#"{
  "hooks": {
    "PreToolUse": [
      {
        "type": "command",
        "command": "tok hook copilot",
        "cwd": ".",
        "timeout": 5
      }
    ],
    "SessionStart": [
      {
        "type": "command",
        "command": "./.github/hooks/tok-memory-session.sh",
        "cwd": ".",
        "timeout": 10
      }
    ],
    "UserPromptSubmit": [
      {
        "type": "command",
        "command": "./.github/hooks/tok-memory-prompt.sh",
        "cwd": ".",
        "timeout": 10
      }
    ],
    "Stop": [
      {
        "type": "command",
        "command": "./.github/hooks/tok-memory-extract.sh",
        "cwd": ".",
        "timeout": 10
      }
    ]
  }
}
"#;

const COPILOT_INSTRUCTIONS: &str = r#"# TOK — Token-Optimized CLI

**tok** is a CLI proxy that filters and compresses command outputs, saving 60-90% tokens. Always prefix shell commands with `tok`. If TOK has a filter, it compresses; if not, it passes through unchanged.

## Rule

Always prefix shell commands with `tok`:

```bash
# Instead of:              Use:
git status                 tok git status
git log -10                tok git log -10
cargo test                 tok cargo test
docker ps                  tok docker ps
kubectl get pods           tok kubectl pods
```

Even in chains: `tok git add . && tok git commit -m "msg" && tok git push`

## Filters (60+ tools)

```bash
# Git & GitHub
tok git status / log / diff / show / add / commit / push / pull / branch / fetch / stash
tok gh pr view <n> / gh issue list / gh run list

# Build & Compile
tok cargo build / check / clippy
tok tsc / lint / prettier --check . / next build
tok dotnet build / go build / go vet

# Test
tok cargo test / vitest run / playwright test / pytest
tok go test ./... / rspec / rake test / dotnet test
tok test <cmd>

# Package Managers
tok pnpm install / list / outdated
tok npm run <script> / npx <cmd> / pip install / deps

# Files & Search
tok ls / tree / read <file> / smart <file>
tok find / grep / wc / diff / json / env / log
tok err <cmd> / summary <cmd>

# Infrastructure & Network
tok docker ps / images / logs / compose
tok kubectl pods / services / logs
tok aws <cmd> / psql <cmd>
tok curl <url> / wget <url>

# Linting & Formatting
tok ruff check . / mypy . / rubocop / golangci-lint run / format

# Stacked PRs (Graphite)
tok gt log / submit / sync / restack / create / branch
```

## Code Graph — try these before reading files

```bash
tok mem ask "<question>"   # Ranked symbols with source inlined. Start here.
tok mem skeleton <file>    # Every signature, no bodies
tok mem grep "<pattern>"   # Text search grouped by enclosing symbol
tok mem map                # Layout, hubs, entry points
tok mem check              # Drift between .tok/map/ and the code
```

Also on MCP as `tok_ask`, `tok_skeleton`, `tok_grep`, `tok_map`,
`tok_relations`, `tok_check`.

## Code Intelligence

```bash
tok mem index <dir>        # Index symbols and structure
tok mem index --deep       # Plus LLM summaries (opt-in)
tok mem search <query>     # Full-text search (BM25)
tok mem find <symbol>      # Exact or fuzzy symbol lookup
tok mem context <symbol>   # Callers, callees, type refs
tok mem impact <symbol>    # Blast radius analysis
tok mem dead-code          # Zero-reference symbols
tok mem changes            # What changed since last session
tok forgemap init          # Annotate source files with headers
tok forgemap check         # Coverage report
tok forgemap manifest      # Generate project manifest
tok forgemap wiki bootstrap  # Emit Obsidian vault
```

## Security

```bash
tok --security <cmd>       # Obfuscate sensitive data in output
tok security-inspect <text>  # Dry-run: inspect for secrets
tok doctor --slm           # Check SLM runtime health
```

## Analytics & Insights

```bash
tok gain                   # Token savings dashboard
tok gain --graph           # ASCII graph of daily savings
tok gain --history         # Per-command savings history
tok cc-economics           # Claude spend vs tok savings
tok discover               # Find missed tok opportunities
tok session                # Usage stats across sessions
tok learn                  # Learn CLI fixes from past mistakes
```

## Configuration

```bash
tok config                 # View or scaffold tok config
tok verify                 # Sanity-check hooks and filters
tok trust / untrust        # Manage local .tok filter trust
tok proxy <cmd>            # Raw passthrough (still tracks stats)
tok man                    # Full command manual (every command)
tok man <topic>            # Filter manual (e.g. tok man security)
```

## Useful Flags

- `-u` / `--ultra-compact` — maximum compression
- `--security` / `--security-mode <mode>` — privacy layer
- `-v` / `-vv` / `-vvv` — increase verbosity

## Verification

```bash
tok --version              # Should show: tok X.Y.Z
tok gain                   # Should work (not "command not found")
which tok                  # Verify correct binary
```

> **Different tool**: If `tok gain` fails, confirm you installed Token Optimization Kit (MantisWare/tok). Rust Type Kit is `reachingforthejack/rtk` (usually the `rtk` command).
"#;

/// Entry point for `tok init --copilot`
pub fn run_copilot(verbose: u8) -> Result<()> {
    // Install in current project's .github/ directory
    let github_dir = Path::new(".github");
    let hooks_dir = github_dir.join("hooks");

    fs::create_dir_all(&hooks_dir).context("Failed to create .github/hooks/ directory")?;

    install_memory_hook_scripts(&hooks_dir, "copilot", verbose)?;

    // 1. Write hook config
    let hook_path = hooks_dir.join("tok-rewrite.json");
    write_if_changed(
        &hook_path,
        COPILOT_HOOK_JSON,
        "Copilot hook config",
        verbose,
    )?;

    // 2. Write instructions
    let instructions_path = github_dir.join("copilot-instructions.md");
    write_if_changed(
        &instructions_path,
        COPILOT_INSTRUCTIONS,
        "Copilot instructions",
        verbose,
    )?;

    println!("\nGitHub Copilot integration installed (project-scoped).\n");
    println!("  Hook config:    {}", hook_path.display());
    println!("  Instructions:   {}", instructions_path.display());
    println!("\n  Works with VS Code Copilot Chat (transparent rewrite)");
    println!("  and Copilot CLI (deny-with-suggestion).");
    println!("\n  Restart your IDE or Copilot CLI session to activate.\n");

    Ok(())
}

// ─── Agent detection for welcome screen ───────────────────────────────

/// Installation status for a single agent.
pub struct AgentStatus {
    pub name: &'static str,
    pub installed: bool,
    pub detail: String,
}

/// Detect installation status for all supported agents.
/// Used by the welcome screen (`tok` with no args) and `tok init --show`.
pub fn detect_agent_statuses() -> Vec<AgentStatus> {
    vec![
        detect_claude(),
        detect_cursor(),
        detect_codex(),
        detect_gemini(),
        detect_opencode(),
        detect_windsurf(),
        detect_cline(),
        detect_copilot(),
    ]
}

fn detect_claude() -> AgentStatus {
    let installed = resolve_claude_dir()
        .map(|d| d.join(HOOKS_SUBDIR).join(REWRITE_HOOK_FILE))
        .map(|p| p.exists())
        .unwrap_or(false);
    let detail = if installed {
        format!(
            "~/{}/{}/{}",
            super::constants::CLAUDE_DIR,
            HOOKS_SUBDIR,
            REWRITE_HOOK_FILE
        )
    } else {
        "not installed".to_string()
    };
    AgentStatus {
        name: "Claude Code",
        installed,
        detail,
    }
}

fn detect_cursor() -> AgentStatus {
    let installed = resolve_cursor_dir()
        .map(|d| d.join(HOOKS_SUBDIR).join(REWRITE_HOOK_FILE))
        .map(|p| p.exists())
        .unwrap_or(false);
    let detail = if installed {
        format!(
            "~/{}/{}/{}",
            super::constants::CURSOR_DIR,
            HOOKS_SUBDIR,
            REWRITE_HOOK_FILE
        )
    } else {
        "not installed".to_string()
    };
    AgentStatus {
        name: "Cursor",
        installed,
        detail,
    }
}

fn detect_codex() -> AgentStatus {
    let installed = resolve_codex_dir()
        .ok()
        .map(|d| d.join("AGENTS.md"))
        .and_then(|p| fs::read_to_string(p).ok())
        .map(|c| c.contains("@TOK.md"))
        .unwrap_or(false);
    let detail = if installed {
        format!("~/{}/AGENTS.md", super::constants::CODEX_DIR)
    } else {
        "not installed".to_string()
    };
    AgentStatus {
        name: "Codex",
        installed,
        detail,
    }
}

fn detect_gemini() -> AgentStatus {
    let installed = resolve_gemini_dir()
        .map(|d| d.join(HOOKS_SUBDIR).join(GEMINI_HOOK_FILE))
        .map(|p| p.exists())
        .unwrap_or(false);
    let detail = if installed {
        format!(
            "~/{}/{}/{}",
            super::constants::GEMINI_DIR,
            HOOKS_SUBDIR,
            GEMINI_HOOK_FILE
        )
    } else {
        "not installed".to_string()
    };
    AgentStatus {
        name: "Gemini CLI",
        installed,
        detail,
    }
}

fn detect_opencode() -> AgentStatus {
    let installed = user_opencode_plugin_path()
        .map(|p| p.exists())
        .unwrap_or(false);
    let detail = if installed {
        format!("~/{}", super::constants::OPENCODE_PLUGIN_PATH)
    } else {
        "not installed".to_string()
    };
    AgentStatus {
        name: "OpenCode",
        installed,
        detail,
    }
}

fn detect_windsurf() -> AgentStatus {
    let path = PathBuf::from(".windsurfrules");
    let installed = fs::read_to_string(&path)
        .map(|c| c.contains("TOK") || c.contains("tok"))
        .unwrap_or(false);
    let detail = if installed {
        ".windsurfrules".to_string()
    } else {
        "not installed (project-local)".to_string()
    };
    AgentStatus {
        name: "Windsurf",
        installed,
        detail,
    }
}

fn detect_cline() -> AgentStatus {
    let path = PathBuf::from(".clinerules");
    let installed = fs::read_to_string(&path)
        .map(|c| c.contains("TOK") || c.contains("tok"))
        .unwrap_or(false);
    let detail = if installed {
        ".clinerules".to_string()
    } else {
        "not installed (project-local)".to_string()
    };
    AgentStatus {
        name: "Cline",
        installed,
        detail,
    }
}

fn detect_copilot() -> AgentStatus {
    let path = PathBuf::from(".github/copilot-instructions.md");
    let installed = fs::read_to_string(&path)
        .map(|c| c.contains("tok"))
        .unwrap_or(false);
    let detail = if installed {
        ".github/copilot-instructions.md".to_string()
    } else {
        "not installed (project-local)".to_string()
    };
    AgentStatus {
        name: "Copilot",
        installed,
        detail,
    }
}

// ─── Global init (all agents) ─────────────────────────────────────────

/// Install TOK hooks for all supported agents in one shot.
/// Errors per agent are reported as warnings; the run continues.
pub fn run_all(verbose: u8, graph: bool) -> Result<()> {
    println!("Installing TOK for all supported agents...\n");

    let mut ok = 0u8;
    let mut failed = 0u8;

    macro_rules! try_agent {
        ($label:expr, $body:expr) => {
            match $body {
                Ok(()) => ok += 1,
                Err(e) => {
                    eprintln!("  [warn] {}: {:#}", $label, e);
                    failed += 1;
                }
            }
        };
    }

    // Claude Code (global, auto-patch)
    try_agent!(
        "Claude Code",
        run(
            true,  // global
            true,  // install_claude
            false, // install_opencode (handled separately)
            false, // install_cursor (handled separately)
            false, // install_windsurf (handled separately)
            false, // install_cline (handled separately)
            false, // claude_md
            false, // hook_only
            false, // codex (handled separately)
            PatchMode::Auto,
            verbose,
            graph,
        )
    );

    // Cursor (global)
    try_agent!("Cursor", install_cursor_hooks(verbose, graph));

    // Codex (global)
    try_agent!("Codex", run_codex_mode(true, verbose));

    // Gemini CLI (global, auto-patch)
    try_agent!(
        "Gemini CLI",
        run_gemini(true, false, PatchMode::Auto, verbose)
    );

    // OpenCode (global)
    try_agent!("OpenCode", run_opencode_only_mode(verbose));

    // Copilot (project-local)
    try_agent!("Copilot", run_copilot(verbose));

    // Windsurf (project-local)
    try_agent!("Windsurf", run_windsurf_mode(verbose));

    // Cline (project-local)
    try_agent!("Cline", run_cline_mode(verbose));

    println!();
    if failed == 0 {
        println!("All {} agents configured successfully.", ok);
    } else {
        println!(
            "{} agents configured, {} failed (see warnings above).",
            ok, failed
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_init_mentions_all_top_level_commands() {
        for cmd in [
            "tok cargo",
            "tok gh",
            "tok vitest",
            "tok tsc",
            "tok lint",
            "tok prettier",
            "tok next",
            "tok playwright",
            "tok prisma",
            "tok pnpm",
            "tok npm",
            "tok curl",
            "tok git",
            "tok docker",
            "tok kubectl",
        ] {
            assert!(
                TOK_INSTRUCTIONS.contains(cmd),
                "Missing {cmd} in TOK_INSTRUCTIONS"
            );
        }
    }

    #[test]
    fn test_init_has_version_marker() {
        assert!(
            TOK_INSTRUCTIONS.contains("<!-- tok-instructions"),
            "TOK_INSTRUCTIONS must have version marker for idempotency"
        );
    }

    #[test]
    fn test_hook_has_guards() {
        assert!(REWRITE_HOOK.contains("command -v tok"));
        assert!(REWRITE_HOOK.contains("command -v jq"));
        // Guards (tok/jq availability checks) must appear before the actual delegation call.
        // The thin delegating hook no longer uses set -euo pipefail.
        let jq_pos = REWRITE_HOOK.find("command -v jq").unwrap();
        let tok_delegate_pos = REWRITE_HOOK.find("tok rewrite \"$CMD\"").unwrap();
        assert!(
            jq_pos < tok_delegate_pos,
            "Guards must appear before tok rewrite delegation"
        );
    }

    #[test]
    fn test_migration_removes_old_block() {
        let input = r#"# My Config

<!-- tok-instructions v2 -->
OLD TOK STUFF
<!-- /tok-instructions -->

More content"#;

        let (result, migrated) = remove_tok_block(input);
        assert!(migrated);
        assert!(!result.contains("OLD TOK STUFF"));
        assert!(result.contains("# My Config"));
        assert!(result.contains("More content"));
    }

    #[test]
    fn test_opencode_plugin_install_and_update() {
        let temp = TempDir::new().unwrap();
        let opencode_dir = temp.path().join("opencode");
        let plugin_path = opencode_dir.join("plugins").join("tok.ts");

        fs::create_dir_all(plugin_path.parent().unwrap()).unwrap();
        assert!(!plugin_path.exists());

        let changed = ensure_opencode_plugin_installed(&plugin_path, 0).unwrap();
        assert!(changed);
        let content = fs::read_to_string(&plugin_path).unwrap();
        assert_eq!(content, OPENCODE_PLUGIN);

        fs::write(&plugin_path, "// old").unwrap();
        let changed_again = ensure_opencode_plugin_installed(&plugin_path, 0).unwrap();
        assert!(changed_again);
        let content_updated = fs::read_to_string(&plugin_path).unwrap();
        assert_eq!(content_updated, OPENCODE_PLUGIN);
    }

    #[test]
    fn test_opencode_plugin_remove() {
        let temp = TempDir::new().unwrap();
        let opencode_dir = temp.path().join("opencode");
        let plugin_path = opencode_dir.join("plugins").join("tok.ts");
        fs::create_dir_all(plugin_path.parent().unwrap()).unwrap();
        fs::write(&plugin_path, OPENCODE_PLUGIN).unwrap();

        assert!(plugin_path.exists());
        fs::remove_file(&plugin_path).unwrap();
        assert!(!plugin_path.exists());
    }

    #[test]
    fn test_migration_warns_on_missing_end_marker() {
        let input = "<!-- tok-instructions v2 -->\nOLD STUFF\nNo end marker";
        let (result, migrated) = remove_tok_block(input);
        assert!(!migrated);
        assert_eq!(result, input);
    }

    #[test]
    #[cfg(unix)]
    fn test_default_mode_creates_hook_and_tok_md() {
        let temp = TempDir::new().unwrap();
        let hook_path = temp.path().join("tok-rewrite.sh");
        let tok_md_path = temp.path().join("TOK.md");

        fs::write(&hook_path, REWRITE_HOOK).unwrap();
        fs::write(&tok_md_path, TOK_SLIM).unwrap();

        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755)).unwrap();

        assert!(hook_path.exists());
        assert!(tok_md_path.exists());

        let metadata = fs::metadata(&hook_path).unwrap();
        assert!(metadata.permissions().mode() & 0o111 != 0);
    }

    #[test]
    fn test_claude_md_mode_creates_full_injection() {
        // Just verify TOK_INSTRUCTIONS constant has the right content
        assert!(TOK_INSTRUCTIONS.contains("<!-- tok-instructions"));
        assert!(TOK_INSTRUCTIONS.contains("tok cargo test"));
        assert!(TOK_INSTRUCTIONS.contains("<!-- /tok-instructions -->"));
        assert!(TOK_INSTRUCTIONS.len() > 4000);
    }

    // --- upsert_tok_block tests ---

    #[test]
    fn test_upsert_tok_block_appends_when_missing() {
        let input = "# Team instructions";
        let (content, action) = upsert_tok_block(input, TOK_INSTRUCTIONS);
        assert_eq!(action, TokBlockUpsert::Added);
        assert!(content.contains("# Team instructions"));
        assert!(content.contains("<!-- tok-instructions"));
    }

    #[test]
    fn test_upsert_tok_block_updates_stale_block() {
        let input = r#"# Team instructions

<!-- tok-instructions v1 -->
OLD TOK CONTENT
<!-- /tok-instructions -->

More notes
"#;

        let (content, action) = upsert_tok_block(input, TOK_INSTRUCTIONS);
        assert_eq!(action, TokBlockUpsert::Updated);
        assert!(!content.contains("OLD TOK CONTENT"));
        assert!(content.contains("tok cargo test")); // from current TOK_INSTRUCTIONS
        assert!(content.contains("# Team instructions"));
        assert!(content.contains("More notes"));
    }

    #[test]
    fn test_upsert_tok_block_noop_when_already_current() {
        let input = format!(
            "# Team instructions\n\n{}\n\nMore notes\n",
            TOK_INSTRUCTIONS
        );
        let (content, action) = upsert_tok_block(&input, TOK_INSTRUCTIONS);
        assert_eq!(action, TokBlockUpsert::Unchanged);
        assert_eq!(content, input);
    }

    #[test]
    fn test_upsert_tok_block_detects_malformed_block() {
        let input = "<!-- tok-instructions v2 -->\npartial";
        let (content, action) = upsert_tok_block(input, TOK_INSTRUCTIONS);
        assert_eq!(action, TokBlockUpsert::Malformed);
        assert_eq!(content, input);
    }

    #[test]
    fn test_init_is_idempotent() {
        let temp = TempDir::new().unwrap();
        let claude_md = temp.path().join("CLAUDE.md");

        fs::write(&claude_md, "# My stuff\n\n@TOK.md\n").unwrap();

        let content = fs::read_to_string(&claude_md).unwrap();
        let count = content.matches("@TOK.md").count();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_patch_agents_md_adds_reference_once() {
        let temp = TempDir::new().unwrap();
        let agents_md = temp.path().join("AGENTS.md");

        fs::write(&agents_md, "# Team rules\n").unwrap();
        let first_added = patch_agents_md(&agents_md, TOK_MD_REF, 0).unwrap();
        let second_added = patch_agents_md(&agents_md, TOK_MD_REF, 0).unwrap();

        assert!(first_added);
        assert!(!second_added);

        let content = fs::read_to_string(&agents_md).unwrap();
        assert_eq!(content.matches("@TOK.md").count(), 1);
    }

    #[test]
    fn test_codex_mode_rejects_auto_patch() {
        let err = run(
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            true,
            PatchMode::Auto,
            0,
            true,
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "--codex cannot be combined with --auto-patch"
        );
    }

    #[test]
    fn test_codex_mode_rejects_no_patch() {
        let err = run(
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            true,
            PatchMode::Skip,
            0,
            true,
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "--codex cannot be combined with --no-patch"
        );
    }

    #[test]
    fn test_patch_agents_md_creates_missing_file() {
        let temp = TempDir::new().unwrap();
        let agents_md = temp.path().join("AGENTS.md");

        let added = patch_agents_md(&agents_md, TOK_MD_REF, 0).unwrap();

        assert!(added);
        let content = fs::read_to_string(&agents_md).unwrap();
        assert_eq!(content, "@TOK.md\n");
    }

    #[test]
    fn test_patch_agents_md_migrates_inline_block() {
        let temp = TempDir::new().unwrap();
        let agents_md = temp.path().join("AGENTS.md");
        fs::write(
            &agents_md,
            "# Team rules\n\n<!-- tok-instructions v2 -->\nold\n<!-- /tok-instructions -->\n",
        )
        .unwrap();

        let added = patch_agents_md(&agents_md, TOK_MD_REF, 0).unwrap();

        assert!(added);
        let content = fs::read_to_string(&agents_md).unwrap();
        assert!(!content.contains("old"));
        assert_eq!(content.matches("@TOK.md").count(), 1);
    }

    #[test]
    fn test_uninstall_codex_at_is_idempotent() {
        let temp = TempDir::new().unwrap();
        let codex_dir = temp.path();
        let agents_md = codex_dir.join("AGENTS.md");
        let tok_md = codex_dir.join("TOK.md");

        fs::write(&agents_md, "# Team rules\n\n@TOK.md\n").unwrap();
        fs::write(&tok_md, "codex config").unwrap();

        let removed_first = uninstall_codex_at(codex_dir, 0).unwrap();
        let removed_second = uninstall_codex_at(codex_dir, 0).unwrap();

        assert_eq!(removed_first.len(), 2);
        assert!(removed_second.is_empty());
        assert!(!tok_md.exists());

        let content = fs::read_to_string(&agents_md).unwrap();
        assert!(!content.contains("@TOK.md"));
        assert!(content.contains("# Team rules"));
    }

    #[test]
    fn test_local_init_unchanged() {
        // Local init should use claude-md mode
        let temp = TempDir::new().unwrap();
        let claude_md = temp.path().join("CLAUDE.md");

        fs::write(&claude_md, TOK_INSTRUCTIONS).unwrap();
        let content = fs::read_to_string(&claude_md).unwrap();

        assert!(content.contains("<!-- tok-instructions"));
    }

    // Tests for hook_already_present()
    #[test]
    fn test_hook_already_present_exact_match() {
        let json_content = serde_json::json!({
            "hooks": {
                "PreToolUse": [{
                    "matcher": "Bash",
                    "hooks": [{
                        "type": "command",
                        "command": "/Users/test/.claude/hooks/tok-rewrite.sh"
                    }]
                }]
            }
        });

        let hook_command = "/Users/test/.claude/hooks/tok-rewrite.sh";
        assert!(hook_already_present(&json_content, hook_command));
    }

    #[test]
    fn test_hook_already_present_different_path() {
        let json_content = serde_json::json!({
            "hooks": {
                "PreToolUse": [{
                    "matcher": "Bash",
                    "hooks": [{
                        "type": "command",
                        "command": "/home/user/.claude/hooks/tok-rewrite.sh"
                    }]
                }]
            }
        });

        let hook_command = "~/.claude/hooks/tok-rewrite.sh";
        // Should match on tok-rewrite.sh substring
        assert!(hook_already_present(&json_content, hook_command));
    }

    #[test]
    fn test_hook_not_present_empty() {
        let json_content = serde_json::json!({});
        let hook_command = "/Users/test/.claude/hooks/tok-rewrite.sh";
        assert!(!hook_already_present(&json_content, hook_command));
    }

    #[test]
    fn test_hook_not_present_other_hooks() {
        let json_content = serde_json::json!({
            "hooks": {
                "PreToolUse": [{
                    "matcher": "Bash",
                    "hooks": [{
                        "type": "command",
                        "command": "/some/other/hook.sh"
                    }]
                }]
            }
        });

        let hook_command = "/Users/test/.claude/hooks/tok-rewrite.sh";
        assert!(!hook_already_present(&json_content, hook_command));
    }

    // Tests for insert_hook_entry()
    #[test]
    fn test_insert_hook_entry_empty_root() {
        let mut json_content = serde_json::json!({});
        let hook_command = "/Users/test/.claude/hooks/tok-rewrite.sh";

        insert_hook_entry(&mut json_content, hook_command);

        // Should create full structure
        assert!(json_content.get("hooks").is_some());
        assert!(json_content
            .get("hooks")
            .unwrap()
            .get("PreToolUse")
            .is_some());

        let pre_tool_use = json_content["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre_tool_use.len(), 1);

        let command = pre_tool_use[0]["hooks"][0]["command"].as_str().unwrap();
        assert_eq!(command, hook_command);
    }

    #[test]
    fn test_insert_hook_entry_preserves_existing() {
        let mut json_content = serde_json::json!({
            "hooks": {
                "PreToolUse": [{
                    "matcher": "Bash",
                    "hooks": [{
                        "type": "command",
                        "command": "/some/other/hook.sh"
                    }]
                }]
            }
        });

        let hook_command = "/Users/test/.claude/hooks/tok-rewrite.sh";
        insert_hook_entry(&mut json_content, hook_command);

        let pre_tool_use = json_content["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre_tool_use.len(), 2); // Should have both hooks

        // Check first hook is preserved
        let first_command = pre_tool_use[0]["hooks"][0]["command"].as_str().unwrap();
        assert_eq!(first_command, "/some/other/hook.sh");

        // Check second hook is TOK
        let second_command = pre_tool_use[1]["hooks"][0]["command"].as_str().unwrap();
        assert_eq!(second_command, hook_command);
    }

    #[test]
    fn test_insert_hook_preserves_other_keys() {
        let mut json_content = serde_json::json!({
            "env": {"PATH": "/custom/path"},
            "permissions": {"allowAll": true},
            "model": "claude-sonnet-4"
        });

        let hook_command = "/Users/test/.claude/hooks/tok-rewrite.sh";
        insert_hook_entry(&mut json_content, hook_command);

        // Should preserve all other keys
        assert_eq!(json_content["env"]["PATH"], "/custom/path");
        assert_eq!(json_content["permissions"]["allowAll"], true);
        assert_eq!(json_content["model"], "claude-sonnet-4");

        // And add hooks
        assert!(json_content.get("hooks").is_some());
    }

    // Tests for atomic_write()
    #[test]
    fn test_atomic_write() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("test.json");

        let content = r#"{"key": "value"}"#;
        atomic_write(&file_path, content).unwrap();

        assert!(file_path.exists());
        let written = fs::read_to_string(&file_path).unwrap();
        assert_eq!(written, content);
    }

    // Test for preserve_order round-trip
    #[test]
    fn test_preserve_order_round_trip() {
        let original = r#"{"env": {"PATH": "/usr/bin"}, "permissions": {"allowAll": true}, "model": "claude-sonnet-4"}"#;
        let parsed: serde_json::Value = serde_json::from_str(original).unwrap();
        let serialized = serde_json::to_string(&parsed).unwrap();

        // Keys should appear in same order
        let _original_keys: Vec<&str> = original.split("\"").filter(|s| s.contains(":")).collect();
        let _serialized_keys: Vec<&str> =
            serialized.split("\"").filter(|s| s.contains(":")).collect();

        // Just check that keys exist (preserve_order doesn't guarantee exact order in nested objects)
        assert!(serialized.contains("\"env\""));
        assert!(serialized.contains("\"permissions\""));
        assert!(serialized.contains("\"model\""));
    }

    // Tests for clean_double_blanks()
    #[test]
    fn test_clean_double_blanks() {
        // Input: line1, 2 blank lines, line2, 1 blank line, line3, 3 blank lines, line4
        // Expected: line1, 2 blank lines (kept), line2, 1 blank line, line3, 2 blank lines (max), line4
        let input = "line1\n\n\nline2\n\nline3\n\n\n\nline4";
        // That's: line1 \n \n \n line2 \n \n line3 \n \n \n \n line4
        // Which is: line1, blank, blank, line2, blank, line3, blank, blank, blank, line4
        // So 2 blanks after line1 (keep both), 1 blank after line2 (keep), 3 blanks after line3 (keep 2)
        let expected = "line1\n\n\nline2\n\nline3\n\n\nline4";
        assert_eq!(clean_double_blanks(input), expected);
    }

    #[test]
    fn test_clean_double_blanks_preserves_single() {
        let input = "line1\n\nline2\n\nline3";
        assert_eq!(clean_double_blanks(input), input); // No change
    }

    // Tests for remove_hook_from_settings()
    #[test]
    fn test_remove_hook_from_json() {
        let mut json_content = serde_json::json!({
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [{
                            "type": "command",
                            "command": "/some/other/hook.sh"
                        }]
                    },
                    {
                        "matcher": "Bash",
                        "hooks": [{
                            "type": "command",
                            "command": "/Users/test/.claude/hooks/tok-rewrite.sh"
                        }]
                    }
                ]
            }
        });

        let removed = remove_hook_from_json(&mut json_content);
        assert!(removed);

        // Should have only one hook left
        let pre_tool_use = json_content["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre_tool_use.len(), 1);

        // Check it's the other hook
        let command = pre_tool_use[0]["hooks"][0]["command"].as_str().unwrap();
        assert_eq!(command, "/some/other/hook.sh");
    }

    #[test]
    fn test_remove_hook_when_not_present() {
        let mut json_content = serde_json::json!({
            "hooks": {
                "PreToolUse": [{
                    "matcher": "Bash",
                    "hooks": [{
                        "type": "command",
                        "command": "/some/other/hook.sh"
                    }]
                }]
            }
        });

        let removed = remove_hook_from_json(&mut json_content);
        assert!(!removed);
    }

    // ─── Cursor hooks.json tests ───

    #[test]
    fn test_cursor_hook_entry_present_rewrite() {
        let json_content = serde_json::json!({
            "version": 1,
            "hooks": {
                "preToolUse": [{
                    "command": "./hooks/tok-rewrite.sh",
                    "matcher": "Shell"
                }]
            }
        });
        assert!(cursor_hook_entry_present(
            &json_content,
            "preToolUse",
            REWRITE_HOOK_FILE
        ));
        assert!(!cursor_hook_entry_present(
            &json_content,
            "sessionStart",
            SESSION_HOOK_FILE
        ));
    }

    #[test]
    fn test_cursor_hook_entry_present_session() {
        let json_content = serde_json::json!({
            "version": 1,
            "hooks": {
                "sessionStart": [{
                    "command": "./hooks/tok-session.sh"
                }]
            }
        });
        assert!(cursor_hook_entry_present(
            &json_content,
            "sessionStart",
            SESSION_HOOK_FILE
        ));
        assert!(!cursor_hook_entry_present(
            &json_content,
            "preToolUse",
            REWRITE_HOOK_FILE
        ));
    }

    #[test]
    fn test_cursor_hook_entry_present_false_empty() {
        let json_content = serde_json::json!({ "version": 1 });
        assert!(!cursor_hook_entry_present(
            &json_content,
            "preToolUse",
            REWRITE_HOOK_FILE
        ));
        assert!(!cursor_hook_entry_present(
            &json_content,
            "sessionStart",
            SESSION_HOOK_FILE
        ));
    }

    #[test]
    fn test_cursor_hook_entry_present_false_other_hooks() {
        let json_content = serde_json::json!({
            "version": 1,
            "hooks": {
                "preToolUse": [{
                    "command": "./hooks/some-other-hook.sh",
                    "matcher": "Shell"
                }]
            }
        });
        assert!(!cursor_hook_entry_present(
            &json_content,
            "preToolUse",
            REWRITE_HOOK_FILE
        ));
    }

    #[test]
    fn test_insert_cursor_hook_array_entry_empty() {
        let mut json_content = serde_json::json!({ "version": 1 });
        insert_cursor_hook_array_entry(
            &mut json_content,
            "preToolUse",
            serde_json::json!({
                "command": format!("./hooks/{}", REWRITE_HOOK_FILE),
                "matcher": "Shell"
            }),
        );

        let hooks = json_content["hooks"]["preToolUse"].as_array().unwrap();
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0]["command"], "./hooks/tok-rewrite.sh");
        assert_eq!(hooks[0]["matcher"], "Shell");
        assert_eq!(json_content["version"], 1);
    }

    #[test]
    fn test_insert_cursor_hook_preserves_existing() {
        let mut json_content = serde_json::json!({
            "version": 1,
            "hooks": {
                "preToolUse": [{
                    "command": "./hooks/other.sh",
                    "matcher": "Shell"
                }],
                "afterFileEdit": [{
                    "command": "./hooks/format.sh"
                }]
            }
        });

        insert_cursor_hook_array_entry(
            &mut json_content,
            "preToolUse",
            serde_json::json!({
                "command": format!("./hooks/{}", REWRITE_HOOK_FILE),
                "matcher": "Shell"
            }),
        );

        let pre_tool_use = json_content["hooks"]["preToolUse"].as_array().unwrap();
        assert_eq!(pre_tool_use.len(), 2);
        assert_eq!(pre_tool_use[0]["command"], "./hooks/other.sh");
        assert_eq!(pre_tool_use[1]["command"], "./hooks/tok-rewrite.sh");

        assert!(json_content["hooks"]["afterFileEdit"].is_array());
    }

    #[test]
    fn test_insert_cursor_session_start_entry() {
        let mut json_content = serde_json::json!({ "version": 1 });
        insert_cursor_hook_array_entry(
            &mut json_content,
            "sessionStart",
            serde_json::json!({
                "command": format!("./hooks/{}", SESSION_HOOK_FILE)
            }),
        );

        let arr = json_content["hooks"]["sessionStart"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["command"], "./hooks/tok-session.sh");
    }

    #[test]
    fn test_remove_cursor_hooks_from_json() {
        let mut json_content = serde_json::json!({
            "version": 1,
            "hooks": {
                "preToolUse": [
                    { "command": "./hooks/other.sh", "matcher": "Shell" },
                    { "command": "./hooks/tok-rewrite.sh", "matcher": "Shell" }
                ],
                "sessionStart": [
                    { "command": "./hooks/tok-session.sh" }
                ]
            }
        });

        let removed = remove_cursor_hooks_from_json(&mut json_content);
        assert!(removed);

        let hooks = json_content["hooks"]["preToolUse"].as_array().unwrap();
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0]["command"], "./hooks/other.sh");

        let session = json_content["hooks"]["sessionStart"].as_array().unwrap();
        assert_eq!(session.len(), 0);
    }

    // --------------------------------------------------- code graph hooks

    #[test]
    fn graph_hooks_register_for_session_start_and_edits() {
        let mut settings = serde_json::json!({});

        let changed = patch_claude_graph_hooks(&mut settings, Path::new("/hooks"));

        assert!(changed);
        assert!(claude_hook_command_present(
            &settings,
            SESSION_START_KEY,
            GRAPH_HOOK_MARKER
        ));
        assert!(claude_hook_command_present(
            &settings,
            POST_TOOL_USE_KEY,
            GRAPH_HOOK_MARKER
        ));
    }

    /// Without a matcher Claude runs the hook after every tool, including reads
    /// and shell commands that cannot have changed a file.
    #[test]
    fn the_post_edit_hook_only_fires_for_editing_tools() {
        let mut settings = serde_json::json!({});
        patch_claude_graph_hooks(&mut settings, Path::new("/hooks"));

        let entry = &settings["hooks"][POST_TOOL_USE_KEY][0];

        assert_eq!(entry["matcher"], EDIT_TOOL_MATCHER);
    }

    /// Lifecycle hooks apply to the session, not to one tool.
    #[test]
    fn the_session_hook_carries_no_matcher() {
        let mut settings = serde_json::json!({});
        patch_claude_graph_hooks(&mut settings, Path::new("/hooks"));

        assert!(settings["hooks"][SESSION_START_KEY][0]
            .get("matcher")
            .is_none());
    }

    #[test]
    fn registering_graph_hooks_twice_adds_nothing() {
        let mut settings = serde_json::json!({});
        patch_claude_graph_hooks(&mut settings, Path::new("/hooks"));

        let changed = patch_claude_graph_hooks(&mut settings, Path::new("/hooks"));

        assert!(!changed);
        assert_eq!(
            settings["hooks"][SESSION_START_KEY]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    /// Both register on SessionStart, and neither may displace the other.
    #[test]
    fn graph_and_memory_session_hooks_coexist() {
        let mut settings = serde_json::json!({});
        let hooks_dir = Path::new("/hooks");

        patch_claude_memory_hooks(&mut settings, hooks_dir);
        patch_claude_graph_hooks(&mut settings, hooks_dir);

        assert!(claude_hook_command_present(
            &settings,
            SESSION_START_KEY,
            MEMORY_HOOK_MARKER
        ));
        assert!(claude_hook_command_present(
            &settings,
            SESSION_START_KEY,
            GRAPH_HOOK_MARKER
        ));
    }

    #[test]
    fn removing_graph_hooks_leaves_memory_and_user_hooks_alone() {
        let mut settings = serde_json::json!({});
        let hooks_dir = Path::new("/hooks");
        patch_claude_memory_hooks(&mut settings, hooks_dir);
        patch_claude_graph_hooks(&mut settings, hooks_dir);
        settings["hooks"][SESSION_START_KEY]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "hooks": [{ "type": "command", "command": "/my/own/hook.sh" }]
            }));

        let removed = remove_graph_hooks_from_claude_json(&mut settings);

        assert!(removed);
        assert!(!claude_hook_command_present(
            &settings,
            SESSION_START_KEY,
            GRAPH_HOOK_MARKER
        ));
        assert!(claude_hook_command_present(
            &settings,
            SESSION_START_KEY,
            MEMORY_HOOK_MARKER
        ));
        assert!(claude_hook_command_present(
            &settings,
            SESSION_START_KEY,
            "/my/own/hook.sh"
        ));
    }

    #[test]
    fn removing_graph_hooks_that_were_never_added_reports_nothing() {
        let mut settings = serde_json::json!({});

        assert!(!remove_graph_hooks_from_claude_json(&mut settings));
    }

    /// The upgrade path: an install predating the graph still has only the
    /// rewrite hook, and re-running `tok init` has to notice that.
    #[test]
    fn an_existing_install_gains_the_hooks_it_predates() {
        let hooks_dir = std::path::Path::new("/home/u/.claude/hooks");
        let rewrite = hooks_dir.join(REWRITE_HOOK_FILE);
        let command = rewrite.to_string_lossy().to_string();

        let mut settings = serde_json::json!({});
        insert_hook_entry(&mut settings, &command);
        let before = settings.clone();

        let added = reconcile_claude_hooks(&mut settings, &command, Some(hooks_dir), true);

        assert!(!added.rewrite, "the rewrite hook was already registered");
        assert!(added.memory, "memory hooks should be added on upgrade");
        assert!(added.graph, "graph hooks should be added on upgrade");
        assert!(added.any());
        assert_ne!(settings, before, "settings were left untouched");
        assert!(claude_hook_command_present(
            &settings,
            POST_TOOL_USE_KEY,
            GRAPH_HOOK_MARKER
        ));
    }

    /// Re-running against a fully configured install must be a no-op, or every
    /// `tok init` would rewrite the file and print a change it did not make.
    #[test]
    fn a_current_install_reconciles_to_no_change() {
        let hooks_dir = std::path::Path::new("/home/u/.claude/hooks");
        let command = hooks_dir
            .join(REWRITE_HOOK_FILE)
            .to_string_lossy()
            .into_owned();

        let mut settings = serde_json::json!({});
        reconcile_claude_hooks(&mut settings, &command, Some(hooks_dir), true);
        let settled = settings.clone();

        let added = reconcile_claude_hooks(&mut settings, &command, Some(hooks_dir), true);

        assert!(!added.any(), "a second pass claimed to add {added:?}");
        assert_eq!(settings, settled);
    }

    /// `--no-graph` must stay honoured on the upgrade path too.
    #[test]
    fn reconciling_without_graph_leaves_the_graph_hooks_out() {
        let hooks_dir = std::path::Path::new("/home/u/.claude/hooks");
        let command = hooks_dir
            .join(REWRITE_HOOK_FILE)
            .to_string_lossy()
            .into_owned();

        let mut settings = serde_json::json!({});
        let added = reconcile_claude_hooks(&mut settings, &command, Some(hooks_dir), false);

        assert!(!added.graph);
        assert!(!claude_hook_command_present(
            &settings,
            POST_TOOL_USE_KEY,
            GRAPH_HOOK_MARKER
        ));
    }

    #[test]
    fn cursor_registers_the_post_edit_hook_on_file_edits() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(HOOKS_JSON);

        patch_cursor_hooks_json(&path, 0, true).expect("patch");

        let root: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("read")).expect("json");
        assert!(cursor_hook_entry_present(
            &root,
            "afterFileEdit",
            GRAPH_POSTEDIT_HOOK_FILE
        ));
    }

    #[test]
    fn no_graph_leaves_the_post_edit_hook_out_but_installs_the_rest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(HOOKS_JSON);

        patch_cursor_hooks_json(&path, 0, false).expect("patch");

        let root: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("read")).expect("json");
        assert!(
            !cursor_hook_entry_present(&root, "afterFileEdit", GRAPH_POSTEDIT_HOOK_FILE),
            "--no-graph still registered the graph post-edit hook"
        );
        assert!(cursor_hook_entry_present(
            &root,
            "preToolUse",
            REWRITE_HOOK_FILE
        ));
    }

    #[test]
    fn cursor_graph_hooks_are_removed_on_uninstall() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(HOOKS_JSON);
        patch_cursor_hooks_json(&path, 0, true).expect("patch");
        let mut root: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("read")).expect("json");

        assert!(remove_cursor_hooks_from_json(&mut root));

        assert!(!cursor_hook_entry_present(
            &root,
            "afterFileEdit",
            GRAPH_POSTEDIT_HOOK_FILE
        ));
    }

    /// The scripts are embedded at compile time, so a rename that breaks the
    /// contract with the CLI should fail the build's tests, not a user's hook.
    #[test]
    fn the_graph_hook_scripts_call_the_commands_they_advertise() {
        assert!(GRAPH_SESSION_HOOK.contains("tok hook graph-session"));
        assert!(GRAPH_POSTEDIT_HOOK.contains("tok hook graph-postedit"));
    }

    /// A post-edit hook that prints is parsed as a hook directive or shown to
    /// the user as noise.
    #[test]
    fn the_post_edit_hook_stays_silent() {
        assert!(GRAPH_POSTEDIT_HOOK.contains(">/dev/null 2>&1 || true"));
    }

    #[test]
    fn test_remove_cursor_hooks_not_present() {
        let mut json_content = serde_json::json!({
            "version": 1,
            "hooks": {
                "preToolUse": [
                    { "command": "./hooks/other.sh", "matcher": "Shell" }
                ]
            }
        });

        let removed = remove_cursor_hooks_from_json(&mut json_content);
        assert!(!removed);
    }

    #[test]
    fn test_cursor_hook_script_has_guards() {
        assert!(CURSOR_REWRITE_HOOK.contains("command -v tok"));
        assert!(CURSOR_REWRITE_HOOK.contains("command -v jq"));
        let jq_pos = CURSOR_REWRITE_HOOK.find("command -v jq").unwrap();
        let tok_delegate_pos = CURSOR_REWRITE_HOOK.find("tok rewrite \"$CMD\"").unwrap();
        assert!(
            jq_pos < tok_delegate_pos,
            "Guards must appear before tok rewrite delegation"
        );
    }

    #[test]
    fn test_cursor_hook_outputs_cursor_format() {
        assert!(CURSOR_REWRITE_HOOK.contains("\"permission\": \"allow\""));
        assert!(CURSOR_REWRITE_HOOK.contains("\"updated_input\""));
        assert!(!CURSOR_REWRITE_HOOK.contains("hookSpecificOutput"));
    }

    #[test]
    fn test_cursor_session_hook_outputs_additional_context() {
        assert!(CURSOR_SESSION_HOOK.contains("additional_context"));
        assert!(CURSOR_SESSION_HOOK.contains("tok gain"));
        assert!(CURSOR_SESSION_HOOK.contains("memory-retrieve"));
        assert!(CURSOR_SESSION_HOOK.contains("tok memory"));
    }
}
