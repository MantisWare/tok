//! Pre-commit hook template and tool prompt content for ForgeMap.
//!
//! Implements FORGEMAP.md §15: a bash pre-commit hook that validates staged files
//! have ForgeMap headers, and tool prompt content for CLAUDE.md / AGENTS.md.

use super::constants::SUPPORTED_EXTENSIONS;

/// Generate the pre-commit hook script content.
pub fn pre_commit_hook_content() -> String {
    let ext_patterns: Vec<String> = SUPPORTED_EXTENSIONS
        .iter()
        .map(|e| format!("*.{}", e))
        .collect();
    let ext_glob = ext_patterns.join("|");

    format!(
        r#"#!/usr/bin/env bash
# ForgeMap pre-commit hook — validates staged files have ForgeMap headers.
# Installed by: tok forgemap install
# Escape hatch: git commit --no-verify

set -e

MISSING=()

while IFS= read -r file; do
  if [ ! -f "$file" ]; then
    continue
  fi
  # Check first 30 lines for ForgeMap field markers.
  if ! head -30 "$file" | grep -qE '^\s*(//|#)\s*(exports:|used_by:|rules:|agent:)'; then
    MISSING+=("$file")
  fi
done < <(git diff --cached --name-only --diff-filter=ACMR | grep -E '\.({ext_glob})$' || true)

if [ ${{#MISSING[@]}} -gt 0 ]; then
  echo ""
  echo "ForgeMap: ${{#MISSING[@]}} staged file(s) missing headers:"
  for f in "${{MISSING[@]}}"; do
    echo "  $f"
  done
  echo ""
  echo "Fix: tok forgemap init <path>"
  echo "Skip: git commit --no-verify"
  exit 1
fi
"#
    )
}

/// Generate the ForgeMap reading protocol content for tool prompt files.
pub fn tool_prompt_content() -> &'static str {
    r#"# ForgeMap Reading Protocol

## Before editing a file
1. Read the module header — the comment block at the top (// or # prefix).
2. Check `used_by:` to find all callers affected by your changes.
3. Check `rules:` for hard constraints before writing logic.
4. Check `related:` for files sharing patterns without importing each other.

## After editing a file
1. Append an `agent:` line (rolling window of last 5 entries).
2. Update `message:` if something is worth flagging for the next agent.
3. Do NOT modify `exports:` or `used_by:` manually — run `tok forgemap refresh`.
"#
}

/// Generate CLAUDE.md ForgeMap section.
pub fn claude_md_section() -> String {
    format!(
        "## ForgeMap\n\n{}\n\nRun `tok forgemap check .` to verify annotation coverage.\n",
        tool_prompt_content()
    )
}

/// Generate AGENTS.md ForgeMap section.
pub fn agents_md_section() -> String {
    format!(
        "## ForgeMap\n\n{}\n\nRun `tok forgemap check .` to verify annotation coverage.\n",
        tool_prompt_content()
    )
}

/// Generate .github/copilot-instructions.md ForgeMap section.
pub fn copilot_instructions_section() -> String {
    format!("## ForgeMap\n\n{}\n", tool_prompt_content())
}
