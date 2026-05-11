//! `tok forgemap install` — install pre-commit hook and tool prompt files.

use std::path::Path;

use anyhow::{Context, Result};
use colored::Colorize;

use crate::forgemap::hook;

/// Run the install command: drop pre-commit hook and tool prompt files.
pub fn run_install(repo_root: &Path, tools: &[String], verbose: bool) -> Result<()> {
    install_pre_commit_hook(repo_root, verbose)?;
    install_tool_prompts(repo_root, tools, verbose)?;
    Ok(())
}

fn install_pre_commit_hook(repo_root: &Path, verbose: bool) -> Result<()> {
    let hooks_dir = repo_root.join(".git/hooks");
    if !hooks_dir.exists() {
        if verbose {
            eprintln!(
                "  {} .git/hooks not found — is this a git repo?",
                "⚠".yellow()
            );
        }
        return Ok(());
    }

    let hook_path = hooks_dir.join("pre-commit");

    if hook_path.exists() {
        // Check if it's already a ForgeMap hook.
        let content = std::fs::read_to_string(&hook_path).unwrap_or_default();
        if content.contains("ForgeMap") {
            if verbose {
                eprintln!("  {} pre-commit hook already installed", "—".dimmed());
            }
            return Ok(());
        }
        if verbose {
            eprintln!(
                "  {} pre-commit hook exists (not ForgeMap) — skipping to avoid overwrite",
                "⚠".yellow()
            );
        }
        return Ok(());
    }

    let content = hook::pre_commit_hook_content();
    std::fs::write(&hook_path, &content)
        .with_context(|| format!("Failed to write pre-commit hook: {}", hook_path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&hook_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&hook_path, perms)?;
    }

    if verbose {
        eprintln!("  {} pre-commit hook installed", "✓".green());
    }
    Ok(())
}

fn install_tool_prompts(repo_root: &Path, tools: &[String], verbose: bool) -> Result<()> {
    let tools_set: Vec<&str> = if tools.is_empty() {
        vec!["claude", "cursor"]
    } else {
        tools.iter().map(|s| s.as_str()).collect()
    };

    for tool in &tools_set {
        match *tool {
            "claude" => {
                write_prompt_file(
                    &repo_root.join("CLAUDE.md"),
                    &hook::claude_md_section(),
                    "CLAUDE.md",
                    verbose,
                )?;
            }
            "cursor" | "codex" => {
                write_prompt_file(
                    &repo_root.join("AGENTS.md"),
                    &hook::agents_md_section(),
                    "AGENTS.md",
                    verbose,
                )?;
            }
            "copilot" => {
                let dir = repo_root.join(".github");
                std::fs::create_dir_all(&dir)?;
                write_prompt_file(
                    &dir.join("copilot-instructions.md"),
                    &hook::copilot_instructions_section(),
                    ".github/copilot-instructions.md",
                    verbose,
                )?;
            }
            other => {
                if verbose {
                    eprintln!("  {} Unknown tool {:?} — skipping", "⚠".yellow(), other);
                }
            }
        }
    }

    Ok(())
}

fn write_prompt_file(path: &Path, section: &str, label: &str, verbose: bool) -> Result<()> {
    if path.exists() {
        let existing = std::fs::read_to_string(path).unwrap_or_default();
        if existing.contains("ForgeMap Reading Protocol") {
            if verbose {
                eprintln!(
                    "  {} {} already contains ForgeMap section",
                    "—".dimmed(),
                    label
                );
            }
            return Ok(());
        }
        // Append section.
        let mut content = existing;
        content.push_str("\n\n");
        content.push_str(section);
        std::fs::write(path, content).with_context(|| format!("Failed to append to {}", label))?;
    } else {
        std::fs::write(path, section).with_context(|| format!("Failed to write {}", label))?;
    }

    if verbose {
        eprintln!("  {} {} installed", "✓".green(), label);
    }
    Ok(())
}
