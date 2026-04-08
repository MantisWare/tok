# TOK install guide (for humans *and* AI assistants)

## ⚠️ Wrong `tok` = wrong tool

Two different projects love the letter T:

1. ✅ **Token Optimization Kit** (this repo) — shrinks CLI output for LLMs  
   - GitHub: `MantisWare/tok`  
   - Binary: **`tok`** — **`tok gain`** must work

2. ❌ **Rust Type Kit** — totally different thing (`reachingforthejack/rtk`, CLI is usually **`rtk`**)  
   - Will not run `tok gain` magic

## Pre-install check (please actually do this)

**Before installing anything:** confirm whether the *right* `tok` is already on the PATH.

```bash
# Check if TOK is installed
tok --version

# CRITICAL: Verify Token Optimization Kit (not a different `tok` binary from crates.io)
tok gain    # Should show token savings stats, NOT "command not found"

# Check installation path
which tok
```

If `tok gain` works, you have the **correct** TOK installed. **DO NOT reinstall**. Skip to "Project Initialization".

If `tok gain` fails but `tok --version` succeeds, you likely have a **different** `tok` binary (e.g. another crates.io package). Uninstall it and install from this repo (see below).

## Installation (only if TOK not available or wrong TOK installed)

### Step 0: Uninstall Wrong TOK (if needed)

If you installed the wrong crate as `tok`:

```bash
cargo uninstall tok
```

(Rust Type Kit is usually the **`rtk`** crate/binary; use `cargo uninstall rtk` only if that applies.)

### Quick Install (Linux/macOS)

```bash
curl -fsSL https://raw.githubusercontent.com/MantisWare/tok/master/install.sh | sh
```

After installation, **verify you have the correct tok**:
```bash
tok gain  # Must show token savings stats (not "command not found")
```

### Alternative: Manual Installation

```bash
# From the MantisWare/tok repository (not Rust Type Kit / reachingforthejack/rtk)
cargo install --git https://github.com/MantisWare/tok

# OR (if published and correct on crates.io)
cargo install tok

# ALWAYS VERIFY after installation
tok gain  # MUST show token savings, not "command not found"
```

⚠️ **WARNING**: `cargo install tok` from crates.io might install the wrong package. Always verify with `tok gain`.

## Project Initialization

### Which mode to choose?

```
  Do you want TOK active across ALL Claude Code projects?
  │
  ├─ YES → tok init -g              (recommended)
  │         Hook + TOK.md (~10 tokens in context)
  │         Commands auto-rewritten transparently
  │
  ├─ YES, minimal → tok init -g --hook-only
  │         Hook only, nothing added to CLAUDE.md
  │         Zero tokens in context
  │
  └─ NO, single project → tok init
            Local CLAUDE.md only (137 lines)
            No hook, no global effect
```

### Recommended: Global Hook-First Setup

**Best for: All projects, automatic TOK usage**

```bash
tok init -g
# → Installs hook to ~/.claude/hooks/tok-rewrite.sh
# → Creates ~/.claude/TOK.md (10 lines, meta commands only)
# → Adds @TOK.md reference to ~/.claude/CLAUDE.md
# → Prompts: "Patch settings.json? [y/N]"
# → If yes: patches + creates backup (~/.claude/settings.json.bak)

# Automated alternatives:
tok init -g --auto-patch    # Patch without prompting
tok init -g --no-patch      # Print manual instructions instead

# Verify installation
tok init --show  # Check hook is installed and executable
```

**Token savings**: ~99.5% reduction (2000 tokens → 10 tokens in context)

**What is settings.json?**
Claude Code's hook registry. TOK adds a PreToolUse hook that rewrites commands transparently. Without this, Claude won't invoke the hook automatically.

```
  Claude Code          settings.json        tok-rewrite.sh        TOK binary
       │                    │                     │                    │
       │  "git status"      │                     │                    │
       │ ──────────────────►│                     │                    │
       │                    │  PreToolUse trigger  │                    │
       │                    │ ───────────────────►│                    │
       │                    │                     │  rewrite command   │
       │                    │                     │  → tok git status  │
       │                    │◄────────────────────│                    │
       │                    │  updated command     │                    │
       │                    │                                          │
       │  execute: tok git status                                      │
       │ ─────────────────────────────────────────────────────────────►│
       │                                                               │  filter
       │  "3 modified, 1 untracked ✓"                                  │
       │◄──────────────────────────────────────────────────────────────│
```

**Backup Safety**:
TOK backs up existing settings.json before changes. Restore if needed:
```bash
cp ~/.claude/settings.json.bak ~/.claude/settings.json
```

### Alternative: Local Project Setup

**Best for: Single project without hook**

```bash
cd /path/to/your/project
tok init  # Creates ./CLAUDE.md with full TOK instructions (137 lines)
```

**Token savings**: Instructions loaded only for this project

### Upgrading from Previous Version

#### From old 137-line CLAUDE.md injection (pre-0.22)

```bash
tok init -g  # Automatically migrates to hook-first mode
# → Removes old 137-line block
# → Installs hook + TOK.md
# → Adds @TOK.md reference
```

#### From old hook with inline logic (pre-0.24) — ⚠️ Breaking Change

TOK 0.24.0 replaced the inline command-detection hook (~200 lines) with a **thin delegator** that calls `tok rewrite`. The binary now contains the rewrite logic, so adding new commands no longer requires a hook update.

The old hook still works but won't benefit from new rules added in future releases.

```bash
# Upgrade hook to thin delegator
tok init --global

# Verify the new hook is active
tok init --show
# Should show: ✅ Hook: ... (thin delegator, up to date)
```

## Common User Flows

### First-Time User (Recommended)
```bash
# 1. Install TOK
cargo install --git https://github.com/MantisWare/tok
tok gain  # Verify (must show token stats)

# 2. Setup with prompts
tok init -g
# → Answer 'y' when prompted to patch settings.json
# → Creates backup automatically

# 3. Restart Claude Code
# 4. Test: git status (should use tok)
```

### CI/CD or Automation
```bash
# Non-interactive setup (no prompts)
tok init -g --auto-patch

# Verify in scripts
tok init --show | grep "Hook:"
```

### Conservative User (Manual Control)
```bash
# Get manual instructions without patching
tok init -g --no-patch

# Review printed JSON snippet
# Manually edit ~/.claude/settings.json
# Restart Claude Code
```

### Temporary Trial
```bash
# Install hook
tok init -g --auto-patch

# Later: remove everything
tok init -g --uninstall

# Restore backup if needed
cp ~/.claude/settings.json.bak ~/.claude/settings.json
```

## Installation Verification

```bash
# Basic test
tok ls .

# Test with git
tok git status

# Test with pnpm (fork only)
tok pnpm list

# Test with Vitest (feat/vitest-support branch only)
tok vitest run
```

## Uninstalling

### Complete Removal (Global Installations Only)

```bash
# Complete removal (global installations only)
tok init -g --uninstall

# What gets removed:
#   - Hook: ~/.claude/hooks/tok-rewrite.sh
#   - Context: ~/.claude/TOK.md
#   - Reference: @TOK.md line from ~/.claude/CLAUDE.md
#   - Registration: TOK hook entry from settings.json

# Restart Claude Code after uninstall
```

**For Local Projects**: Manually remove TOK block from `./CLAUDE.md`

### Binary Removal

```bash
# If installed via cargo
cargo uninstall tok

# If installed via package manager
brew uninstall tok          # macOS Homebrew
sudo apt remove tok         # Debian/Ubuntu
sudo dnf remove tok         # Fedora/RHEL
```

### Restore from Backup (if needed)

```bash
cp ~/.claude/settings.json.bak ~/.claude/settings.json
```

## Essential Commands

### Files
```bash
tok ls .              # Compact tree view
tok read file.rs      # Optimized reading
tok grep "pattern" .  # Grouped search results
```

### Git
```bash
tok git status        # Compact status
tok git log -n 10     # Condensed logs
tok git diff          # Optimized diff
tok git add .         # → "ok ✓"
tok git commit -m "msg"  # → "ok ✓ abc1234"
tok git push          # → "ok ✓ main"
```

### Pnpm (fork only)
```bash
tok pnpm list         # Dependency tree (-70% tokens)
tok pnpm outdated     # Available updates (-80-90%)
tok pnpm install pkg  # Silent installation
```

### Tests
```bash
tok test cargo test   # Failures only (-90%)
tok vitest run        # Filtered Vitest output (-99.6%)
```

### Statistics
```bash
tok gain              # Token savings
tok gain --graph      # With ASCII graph
tok gain --history    # With command history
```

## Validated Token Savings

### Production T3 Stack Project
| Operation | Standard | TOK | Reduction |
|-----------|----------|-----|-----------|
| `vitest run` | 102,199 chars | 377 chars | **-99.6%** |
| `git status` | 529 chars | 217 chars | **-59%** |
| `pnpm list` | ~8,000 tokens | ~2,400 | **-70%** |
| `pnpm outdated` | ~12,000 tokens | ~1,200-2,400 | **-80-90%** |

### Typical Claude Code Session (30 min)
- **Without TOK**: ~150,000 tokens
- **With TOK**: ~45,000 tokens
- **Savings**: **70% reduction**

## Troubleshooting

### TOK command not found after installation
```bash
# Check PATH
echo $PATH | grep -o '[^:]*\.cargo[^:]*'

# Add to PATH if needed (~/.bashrc or ~/.zshrc)
export PATH="$HOME/.cargo/bin:$PATH"

# Reload shell
source ~/.bashrc  # or source ~/.zshrc
```

### TOK command not available (e.g., vitest)
```bash
# Check branch
cd /path/to/tok
git branch

# Switch to feat/vitest-support if needed
git checkout feat/vitest-support

# Reinstall
cargo install --path . --force
```

### Compilation error
```bash
# Update Rust
rustup update stable

# Clean and recompile
cargo clean
cargo build --release
cargo install --path . --force
```

## Support and Contributing

- **Project**: https://github.com/MantisWare/tok
- **Troubleshooting**: See [TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) for common issues
- **GitHub issues**: https://github.com/MantisWare/tok/issues
- **Pull Requests**: https://github.com/MantisWare/tok/pulls

⚠️ **If you installed the wrong tok (Type Kit)**, see [TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md#problem-tok-gain-command-not-found)

## AI Assistant Checklist

Before each session:

- [ ] Verify TOK is installed: `tok --version`
- [ ] If not installed → follow "Install from fork"
- [ ] If project not initialized → `tok init`
- [ ] Use `tok` for ALL git/pnpm/test/vitest commands
- [ ] Check savings: `tok gain`

**Golden Rule**: AI coding assistants should ALWAYS use `tok` as a proxy for shell commands that generate verbose output (git, pnpm, npm, cargo test, vitest, docker, kubectl).
