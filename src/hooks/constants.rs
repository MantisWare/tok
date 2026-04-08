pub const REWRITE_HOOK_FILE: &str = "tok-rewrite.sh";
pub const GEMINI_HOOK_FILE: &str = "tok-hook-gemini.sh";

/// Home-relative path to the OpenCode config directory (`~/.config/opencode`).
#[allow(dead_code)] // Documents layout with `OPENCODE_PLUGIN_PATH`; hook paths use the plugin constant.
pub const OPENCODE_CONFIG_DIR: &str = ".config/opencode";
/// Home-relative path to the OpenCode plugin file (`init` writes here; `hook_check` tests probe it).
pub const OPENCODE_PLUGIN_PATH: &str = ".config/opencode/plugins/tok.ts";
pub const CURSOR_DIR: &str = ".cursor";
pub const CODEX_DIR: &str = ".codex";
pub const GEMINI_DIR: &str = ".gemini";
pub const CLAUDE_DIR: &str = ".claude";
pub const HOOKS_SUBDIR: &str = "hooks";
pub const SETTINGS_JSON: &str = "settings.json";
pub const SETTINGS_LOCAL_JSON: &str = "settings.local.json";
pub const HOOKS_JSON: &str = "hooks.json";
pub const PRE_TOOL_USE_KEY: &str = "PreToolUse";
pub const BEFORE_TOOL_KEY: &str = "BeforeTool";
