//! CLI command handlers for `tok forgemap` — code-indexing and annotation engine.

use std::path::PathBuf;

use anyhow::{Context, Result};
use colored::Colorize;

use crate::forgemap::commands::{
    check, init, install as install_cmd, manifest, refresh, update, wiki_cmd,
};
use crate::forgemap::fmt::gen_session_id;
use crate::forgemap::types::{InitOptions, ManifestOptions, WikiBootstrapOptions};
use crate::{ForgemapCommands, ForgemapWikiCommands};

/// Dispatch a `tok forgemap <subcommand>` to the correct handler.
pub fn dispatch_forgemap(cmd: ForgemapCommands, verbose: u8) -> Result<i32> {
    match cmd {
        ForgemapCommands::Init {
            path,
            repo_root,
            exclude,
            extensions,
            dry_run,
            force,
            model,
            session_id,
        } => {
            let repo_root_path = resolve_repo_root(repo_root.as_deref(), &path)?;
            let target = resolve_target(&path)?;
            let opts = InitOptions {
                target,
                repo_root: repo_root_path,
                extensions: if extensions.is_empty() {
                    None
                } else {
                    Some(extensions)
                },
                exclude,
                dry_run,
                force,
                verbose: verbose > 0,
                model_id: model,
                session_id: session_id.unwrap_or_else(gen_session_id),
            };
            let result = init::run_init(&opts)?;
            let output = init::format_init_result(&result, &path, dry_run);
            println!("{}", output);
            Ok(0)
        }

        ForgemapCommands::Update {
            path,
            repo_root,
            exclude,
            extensions,
            dry_run,
            model,
            session_id,
        } => {
            let repo_root_path = resolve_repo_root(repo_root.as_deref(), &path)?;
            let target = resolve_target(&path)?;
            let opts = InitOptions {
                target,
                repo_root: repo_root_path,
                extensions: if extensions.is_empty() {
                    None
                } else {
                    Some(extensions)
                },
                exclude,
                dry_run,
                force: false,
                verbose: verbose > 0,
                model_id: model,
                session_id: session_id.unwrap_or_else(gen_session_id),
            };
            let result = update::run_update(&opts)?;
            let output = init::format_init_result(&result, &path, dry_run);
            println!("{}", output);
            Ok(0)
        }

        ForgemapCommands::Check {
            path,
            repo_root,
            exclude,
            extensions,
        } => {
            let repo_root_path = resolve_repo_root(repo_root.as_deref(), &path)?;
            let target = resolve_target(&path)?;
            let opts = check::CheckOptions {
                target,
                repo_root: repo_root_path,
                extensions: if extensions.is_empty() {
                    None
                } else {
                    Some(extensions)
                },
                exclude,
                verbose: verbose > 0,
            };
            let result = check::run_check(&opts)?;
            let output = check::format_check_result(&result, &path, verbose > 0);
            println!("{}", output);
            if result.all_annotated {
                Ok(0)
            } else {
                Ok(1)
            }
        }

        ForgemapCommands::Refresh {
            path,
            repo_root,
            exclude,
            extensions,
            dry_run,
        } => {
            let repo_root_path = resolve_repo_root(repo_root.as_deref(), &path)?;
            let target = resolve_target(&path)?;
            let opts = refresh::RefreshOptions {
                target,
                repo_root: repo_root_path,
                extensions: if extensions.is_empty() {
                    None
                } else {
                    Some(extensions)
                },
                exclude,
                dry_run,
                verbose: verbose > 0,
            };
            let result = refresh::run_refresh(&opts)?;
            let output = refresh::format_refresh_result(&result, &path, dry_run);
            println!("{}", output);
            Ok(0)
        }

        ForgemapCommands::Manifest {
            path,
            repo_root,
            exclude,
            extensions,
            dry_run,
            model,
            session_id,
        } => {
            let repo_root_path = resolve_repo_root(repo_root.as_deref(), &path)?;
            let target = resolve_target(&path)?;
            let opts = ManifestOptions {
                target,
                repo_root: repo_root_path.clone(),
                extensions: if extensions.is_empty() {
                    None
                } else {
                    Some(extensions)
                },
                exclude,
                dry_run,
                verbose: verbose > 0,
                model_id: model,
                session_id: session_id.unwrap_or_else(gen_session_id),
            };
            let output = manifest::run_manifest(&opts)?;
            if dry_run {
                println!("{}", output);
            } else {
                println!(
                    "{} .forgemap manifest written to {}",
                    "✓".green(),
                    repo_root_path.join(".forgemap").display()
                );
            }
            Ok(0)
        }

        ForgemapCommands::Wiki { command } => match command {
            ForgemapWikiCommands::Bootstrap {
                path,
                out,
                repo_root,
                exclude,
                extensions,
            } => {
                let repo_root_path = resolve_repo_root(repo_root.as_deref(), &path)?;
                let target = resolve_target(&path)?;
                let opts = WikiBootstrapOptions {
                    target,
                    repo_root: repo_root_path,
                    out_dir: PathBuf::from(&out),
                    extensions: if extensions.is_empty() {
                        None
                    } else {
                        Some(extensions)
                    },
                    exclude,
                    verbose: verbose > 0,
                };
                let (written, skipped) = wiki_cmd::run_wiki_bootstrap(&opts)?;
                println!(
                    "{} Wiki bootstrap: {} pages written, {} skipped (no header)",
                    "✓".green(),
                    written,
                    skipped
                );
                println!("  Output: {}", out);
                Ok(0)
            }
            ForgemapWikiCommands::Sync {
                path,
                out,
                repo_root,
            } => {
                let repo_root_path = resolve_repo_root(repo_root.as_deref(), &path)?;
                let out_path = PathBuf::from(&out);
                wiki_cmd::run_wiki_sync(&repo_root_path, &out_path, verbose > 0)?;
                println!("{} Project wiki synced to {}", "✓".green(), out);
                Ok(0)
            }
        },

        ForgemapCommands::Install { tools } => {
            let repo_root_path = resolve_repo_root(None, ".")?;
            install_cmd::run_install(&repo_root_path, &tools, verbose > 0)?;
            println!("{} ForgeMap install complete", "✓".green());
            Ok(0)
        }
    }
}

/// Resolve the repo root — use provided value, or discover via git, or fall back to target.
fn resolve_repo_root(provided: Option<&str>, target: &str) -> Result<PathBuf> {
    if let Some(root) = provided {
        return std::fs::canonicalize(root)
            .with_context(|| format!("Cannot resolve repo root: {}", root));
    }

    // Try to discover via git rev-parse.
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output();

    if let Ok(out) = output {
        if out.status.success() {
            let root = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !root.is_empty() {
                return Ok(PathBuf::from(root));
            }
        }
    }

    // Fallback: canonicalize target.
    std::fs::canonicalize(target)
        .with_context(|| format!("Cannot resolve target as repo root: {}", target))
}

/// Resolve a target path (canonicalize it).
fn resolve_target(path: &str) -> Result<PathBuf> {
    std::fs::canonicalize(path).with_context(|| format!("Cannot resolve target path: {}", path))
}
