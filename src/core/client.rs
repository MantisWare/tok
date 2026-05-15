//! Client attribution for `tok gain` (Cursor, Claude Code, terminal, etc.).

/// Environment variable set by hooks or prefixed on rewritten commands.
pub const ENV_TOK_CLIENT: &str = "TOK_CLIENT";

/// Default when no client is specified (direct shell invocation).
pub const DEFAULT_CLIENT: &str = "terminal";

/// Label for rows recorded before client tracking existed.
pub const LEGACY_UNKNOWN_CLIENT: &str = "unknown";

/// Resolve the active client id from the process environment.
pub fn resolve_client_id() -> String {
    match std::env::var(ENV_TOK_CLIENT) {
        Ok(value) => normalize_client_id(&value),
        Err(_) => DEFAULT_CLIENT.to_string(),
    }
}

/// Normalize a client id: lowercase, `[a-z0-9_-]` only, max 32 chars.
pub fn normalize_client_id(raw: &str) -> String {
    let trimmed = raw.trim().to_lowercase();
    let sanitized: String = trimmed
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .take(32)
        .collect();
    if sanitized.is_empty() {
        DEFAULT_CLIENT.to_string()
    } else {
        sanitized
    }
}

/// Prefix a rewritten command so the agent shell inherits `TOK_CLIENT`.
pub fn prefix_command_with_client(cmd: &str, client: &str) -> String {
    let client = normalize_client_id(client);
    if client == DEFAULT_CLIENT {
        return cmd.to_string();
    }
    let prefix = format!("{ENV_TOK_CLIENT}={client} ");
    if cmd.starts_with(ENV_TOK_CLIENT) {
        return cmd.to_string();
    }
    format!("{prefix}{cmd}")
}

/// If `TOK_CLIENT` is set in the environment, prefix `rewritten` for hook output.
pub fn apply_hook_client_prefix(rewritten: &str) -> String {
    let client = resolve_client_id();
    if client == DEFAULT_CLIENT {
        rewritten.to_string()
    } else {
        prefix_command_with_client(rewritten, &client)
    }
}

/// Display name for gain tables (legacy empty → unknown).
pub fn display_client_name(client: &str) -> &str {
    if client.is_empty() {
        LEGACY_UNKNOWN_CLIENT
    } else {
        client
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn env_lock() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn test_normalize_client_id() {
        assert_eq!(normalize_client_id("  Cursor  "), "cursor");
        assert_eq!(normalize_client_id("claude-code"), "claude-code");
        assert_eq!(normalize_client_id("!!!"), DEFAULT_CLIENT);
        assert_eq!(normalize_client_id(""), DEFAULT_CLIENT);
    }

    #[test]
    fn test_prefix_command_with_client() {
        let out = prefix_command_with_client("tok git status", "cursor");
        assert_eq!(out, "TOK_CLIENT=cursor tok git status");
        assert_eq!(
            prefix_command_with_client(&out, "cursor"),
            out,
            "double prefix avoided"
        );
    }

    #[test]
    fn test_resolve_client_id_from_env() {
        let _guard = env_lock();
        std::env::set_var(ENV_TOK_CLIENT, "claude");
        assert_eq!(resolve_client_id(), "claude");
        std::env::remove_var(ENV_TOK_CLIENT);
        assert_eq!(resolve_client_id(), DEFAULT_CLIENT);
    }
}
