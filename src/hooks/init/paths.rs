//! Home-relative paths for hook installation targets.

use anyhow::{Context, Result};
use std::path::PathBuf;

use super::super::constants::{
    CLAUDE_DIR, CODEX_DIR, CURSOR_DIR, GEMINI_DIR, OPENCODE_PLUGIN_PATH,
};

fn resolve_home_subdir(subdir: &str) -> Result<PathBuf> {
    dirs::home_dir()
        .map(|h| h.join(subdir))
        .context("Cannot determine home directory. Is $HOME set?")
}

pub(super) fn resolve_claude_dir() -> Result<PathBuf> {
    resolve_home_subdir(CLAUDE_DIR)
}

pub(super) fn resolve_codex_dir() -> Result<PathBuf> {
    resolve_home_subdir(CODEX_DIR)
}

/// User-level OpenCode plugin file (`~/.config/opencode/plugins/tok.ts`).
pub(super) fn user_opencode_plugin_path() -> Result<PathBuf> {
    resolve_home_subdir(OPENCODE_PLUGIN_PATH)
}

pub(super) fn resolve_cursor_dir() -> Result<PathBuf> {
    resolve_home_subdir(CURSOR_DIR)
}

pub(super) fn resolve_gemini_dir() -> Result<PathBuf> {
    resolve_home_subdir(GEMINI_DIR)
}
