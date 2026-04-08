# TOK troubleshooting

Stuff breaks; here’s how to un-break it. Tone: calm, caffeinated engineer.

## “tok gain” isn’t a command (but `tok --version` works)

### Symptom
```bash
$ tok --version
tok 1.0.0  # (or similar)

$ tok gain
tok: 'gain' is not a tok command. See 'tok --help'.
```

### Root Cause
You likely installed a **different Rust crate** that also provides a `tok` binary, or an incomplete build. **Token Optimization Kit** is this project (`tok-ai/tok`). **Rust Type Kit** is unrelated (`reachingforthejack/rtk`, usually the **`rtk`** command).

### Solution

**1. Uninstall the wrong package:**
```bash
cargo uninstall tok
```

**2. Install Token Optimization Kit:**

#### Quick install (Linux/macOS)
```bash
curl -fsSL https://raw.githubusercontent.com/tok-ai/tok/refs/heads/master/install.sh | sh
```

#### Alternative: Manual Installation
```bash
cargo install --git https://github.com/tok-ai/tok
```

**3. Verify installation:**
```bash
tok --version
tok gain  # MUST show token savings stats, not error
```

If `tok gain` now works, installation is correct.

---

## “Which tok is this anyway?”

### The two projects (yes, it’s confusing)

| Project | Repository | Purpose | Typical CLI |
|---------|-----------|---------|-------------|
| **Token Optimization Kit** ✅ | tok-ai/tok | LLM token optimizer for Claude Code | `tok` (`tok gain`) |
| **Rust Type Kit** ❌ | reachingforthejack/rtk | Rust codebase query and type generator | `rtk` (`rtk query`) |

### How to identify Token Optimization Kit

```bash
tok gain   # shows token savings stats when this tool is installed correctly
```

---

## Problem: `cargo install tok` is not Token Optimization Kit

### Why this happens
The crates.io package name `tok` may refer to a **different** crate than this project. Token Optimization Kit is safest installed from this repository.

### Solution
**Do not assume** `cargo install tok` is Token Optimization Kit without verifying.

**Always use explicit repository URLs:**

```bash
# CORRECT - Token Optimization Kit
cargo install --git https://github.com/tok-ai/tok

# OR install from fork
git clone https://github.com/tok-ai/tok.git
cd tok && git checkout feat/all-features
cargo install --path . --force
```

**After any installation, ALWAYS verify:**
```bash
tok gain  # Must work if you want Token Optimization Kit
```

---

## Problem: TOK not working in Claude Code

### Symptom
Claude Code doesn't seem to be using tok, outputs are verbose.

### Checklist

**1. Verify tok is installed and correct:**
```bash
tok --version
tok gain  # Must show stats
```

**2. Initialize tok for Claude Code:**
```bash
# Global (all projects)
tok init --global

# Per-project
cd /your/project
tok init
```

**3. Verify CLAUDE.md file exists:**
```bash
# Check global
cat ~/.claude/CLAUDE.md | grep tok

# Check project
cat ./CLAUDE.md | grep tok
```

**4. Install auto-rewrite hook (recommended for automatic TOK usage):**

**Option A: Automatic (recommended)**
```bash
tok init -g
# → Installs hook + TOK.md automatically
# → Follow printed instructions to add hook to ~/.claude/settings.json
# → Restart Claude Code

# Verify installation
tok init --show  # Should show "✅ Hook: executable, with guards"
```

**Option B: Manual (fallback)**
```bash
# Copy hook to Claude Code hooks directory
mkdir -p ~/.claude/hooks
cp .claude/hooks/tok-rewrite.sh ~/.claude/hooks/
chmod +x ~/.claude/hooks/tok-rewrite.sh
```

Then add to `~/.claude/settings.json` (replace `~` with full path):
```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "/Users/yourname/.claude/hooks/tok-rewrite.sh"
          }
        ]
      }
    ]
  }
}
```

**Note**: Use absolute path in `settings.json`, not `~/.claude/...`

---

## Problem: TOK not working in OpenCode

### Symptom
OpenCode runs commands without tok, outputs are verbose.

### Checklist

**1. Verify tok is installed and correct:**
```bash
tok --version
tok gain  # Must show stats
```

**2. Install the OpenCode plugin (global only):**
```bash
tok init -g --opencode
```

**3. Verify plugin file exists:**
```bash
ls -la ~/.config/opencode/plugins/tok.ts
```

**4. Restart OpenCode**
OpenCode must be restarted to load the plugin.

**5. Verify status:**
```bash
tok init --show  # Should show "OpenCode: plugin installed"
```

---

## Problem: TOK commands fail on Windows ("program not found" or "No such file")

### Symptom
```
tok vitest --run
# Error: program not found
# Or: The system cannot find the file specified

tok lint .
# Error: No such file or directory
```

### Root Cause
On Windows, Node.js tools (vitest, eslint, tsc, etc.) are installed as `.CMD` or `.BAT` wrapper scripts, not as native `.exe` binaries. Rust's `std::process::Command::new("vitest")` does not honor the Windows `PATHEXT` environment variable, so it cannot find `vitest.CMD` even when it's on PATH.

### Solution
Update to tok v0.23.1+ which resolves this via the `which` crate for proper PATH+PATHEXT resolution. All 16+ command modules now use `resolved_command()` instead of `Command::new()`.

```bash
cargo install --git https://github.com/tok-ai/tok
tok --version  # Should be 0.23.1+
```

### Affected Commands
All commands that spawn external tools: `tok vitest`, `tok lint`, `tok tsc`, `tok pnpm`, `tok playwright`, `tok prisma`, `tok next`, `tok prettier`, `tok ruff`, `tok pytest`, `tok pip`, `tok mypy`, `tok golangci-lint`, and others.

---

## Problem: "command not found: tok" after installation

### Symptom
```bash
$ cargo install --path . --force
   Compiling tok v0.7.1
    Finished release [optimized] target(s)
  Installing ~/.cargo/bin/tok

$ tok --version
zsh: command not found: tok
```

### Root Cause
`~/.cargo/bin` is not in your PATH.

### Solution

**1. Check if cargo bin is in PATH:**
```bash
echo $PATH | grep -o '[^:]*\.cargo[^:]*'
```

**2. If not found, add to PATH:**

For **bash** (`~/.bashrc`):
```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

For **zsh** (`~/.zshrc`):
```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

For **fish** (`~/.config/fish/config.fish`):
```fish
set -gx PATH $HOME/.cargo/bin $PATH
```

**3. Reload shell config:**
```bash
source ~/.bashrc  # or ~/.zshrc or restart terminal
```

**4. Verify:**
```bash
which tok
tok --version
tok gain
```

---

## Problem: Compilation errors during installation

### Symptom
```bash
$ cargo install --path .
error: failed to compile tok v0.7.1
```

### Solutions

**1. Update Rust toolchain:**
```bash
rustup update stable
rustup default stable
```

**2. Clean and rebuild:**
```bash
cargo clean
cargo build --release
cargo install --path . --force
```

**3. Check Rust version (minimum required):**
```bash
rustc --version  # Should be 1.70+ for most features
```

**4. If still fails, report issue:**
- GitHub: https://github.com/tok-ai/tok/issues

---

## Need More Help?

**Report issues:**
- Fork-specific: https://github.com/tok-ai/tok/issues
- Upstream: https://github.com/tok-ai/tok/issues

**Run the diagnostic script:**
```bash
# From the tok repository root
bash scripts/check-installation.sh
```

This script will check:
- ✅ TOK installed and in PATH
- ✅ Correct version (Token Killer, not Type Kit)
- ✅ Available features (pnpm, vitest, next, etc.)
- ✅ Claude Code integration (CLAUDE.md files)
- ✅ Auto-rewrite hook status

The script provides specific fix commands for any issues found.
