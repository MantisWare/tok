//! Resolve memory scope from environment and project markers.

use std::path::PathBuf;
use std::process::Command;

use super::config::MemoryScopesConfig;
use super::types::TokMemoryScope;

/// Build scope for the current process context.
pub fn resolve_scope(config: &MemoryScopesConfig) -> TokMemoryScope {
    let user_id = std::env::var("TOK_MEMORY_USER_ID")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| config.default_user_id.clone());

    let project_id = std::env::var("TOK_MEMORY_PROJECT_ID")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            if config.auto_detect_project {
                detect_project_id()
            } else {
                None
            }
        });

    let session_id = std::env::var("TOK_SESSION_ID")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("TOK_MEMORY_SESSION_ID").ok())
        .filter(|s| !s.is_empty());

    let agent_id = std::env::var("TOK_MEMORY_AGENT_ID")
        .ok()
        .filter(|s| !s.is_empty());

    let client_id = std::env::var("TOK_MEMORY_CLIENT_ID")
        .ok()
        .filter(|s| !s.is_empty());

    let workspace_id = std::env::var("TOK_MEMORY_WORKSPACE_ID")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|p| p.display().to_string())
        });

    TokMemoryScope {
        user_id,
        workspace_id,
        project_id,
        agent_id,
        session_id,
        client_id,
    }
}

/// Git root directory name, or cwd basename, or `"default"`.
pub fn detect_project_id() -> Option<String> {
    git_root()
        .map(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(String::from)
                .unwrap_or_else(|| p.display().to_string())
        })
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .and_then(|p| p.file_name().and_then(|n| n.to_str()).map(String::from))
        })
}

fn git_root() -> Option<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

/// Returns true if at least one scope dimension beyond user_id is set.
#[allow(dead_code)]
pub fn has_scope_filter(scope: &TokMemoryScope) -> bool {
    scope.project_id.is_some()
        || scope.session_id.is_some()
        || scope.agent_id.is_some()
        || scope.workspace_id.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_scope_uses_default_user() {
        let config = MemoryScopesConfig::default();
        let scope = resolve_scope(&config);
        assert_eq!(scope.user_id, "local-user");
    }
}
