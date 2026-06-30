//! `tok doctor` — health checks and automated repair for the TOK install.
//!
//! Diagnoses the problems that make TOK "act up": a missing, stale, or
//! tampered rewrite hook (the thing the terminal and Cursor complain about)
//! and failing inline filter self-tests (what `tok verify` reports).
//!
//! With `--repair`, doctor applies safe automated fixes:
//! - **Hook issues** (not installed, orphaned baseline, stale hash, tampered)
//!   are repaired by reinstalling the canonical hook and re-establishing the
//!   integrity baseline — the same action as `tok init -g --auto-patch`.
//!
//! Filter-test failures are *compiled into the installed binary*, so they
//! cannot be patched at runtime. Doctor detects and reports them with clear
//! guidance (update TOK, then re-verify) rather than pretending to fix them.

use anyhow::Result;
use colored::Colorize;

use crate::core::toml_filter;
use crate::hooks::init::{self, PatchMode};
use crate::hooks::integrity::{self, IntegrityStatus};

/// Severity of a single diagnostic check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Health {
    Pass,
    Warn,
    Fail,
}

/// How (if at all) `--repair` can fix a detected problem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Repair {
    /// Reinstall the rewrite hook + re-establish the integrity baseline.
    Hook,
    /// Not auto-repairable at runtime (e.g. a compiled-in filter bug).
    Manual,
}

/// Outcome of one health check.
struct Diagnostic {
    name: &'static str,
    health: Health,
    detail: String,
    /// Guidance printed when the issue is not auto-repairable.
    guidance: Option<String>,
    repair: Repair,
}

impl Diagnostic {
    fn print(&self) {
        let label = match self.health {
            Health::Pass => "PASS".green(),
            Health::Warn => "WARN".yellow(),
            Health::Fail => "FAIL".red(),
        };
        println!("  {}  {}", label, self.name);
        if !self.detail.is_empty() {
            for line in self.detail.lines() {
                println!("        {line}");
            }
        }
    }

    /// True when this check found a problem that `--repair` should act on.
    fn needs_attention(&self) -> bool {
        self.health != Health::Pass
    }
}

/// Check the rewrite hook against its stored SHA-256 baseline.
fn check_hook() -> Diagnostic {
    let hook_path = integrity::resolve_hook_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "~/.claude/hooks/tok-rewrite.sh".to_string());

    match integrity::verify_hook() {
        Ok(IntegrityStatus::Verified) => Diagnostic {
            name: "hook integrity",
            health: Health::Pass,
            detail: format!("verified: {hook_path}"),
            guidance: None,
            repair: Repair::Hook,
        },
        Ok(IntegrityStatus::Tampered { expected, actual }) => Diagnostic {
            name: "hook integrity",
            health: Health::Fail,
            detail: format!(
                "hook modified outside `tok init`\nexpected: {}…\nactual:   {}…",
                expected.get(..16).unwrap_or(&expected),
                actual.get(..16).unwrap_or(&actual),
            ),
            guidance: None,
            repair: Repair::Hook,
        },
        Ok(IntegrityStatus::NoBaseline) => Diagnostic {
            name: "hook integrity",
            health: Health::Warn,
            detail: "hook exists but has no integrity baseline".to_string(),
            guidance: None,
            repair: Repair::Hook,
        },
        Ok(IntegrityStatus::NotInstalled) => Diagnostic {
            name: "hook integrity",
            health: Health::Warn,
            detail: "TOK rewrite hook is not installed".to_string(),
            guidance: None,
            repair: Repair::Hook,
        },
        Ok(IntegrityStatus::OrphanedHash) => Diagnostic {
            name: "hook integrity",
            health: Health::Warn,
            detail: "baseline hash exists but the hook file is missing".to_string(),
            guidance: None,
            repair: Repair::Hook,
        },
        Err(e) => Diagnostic {
            name: "hook integrity",
            health: Health::Fail,
            detail: format!("could not verify hook: {e}"),
            guidance: None,
            repair: Repair::Hook,
        },
    }
}

/// Run every inline filter self-test (the same suite as `tok verify`).
fn check_filters() -> Diagnostic {
    let results = toml_filter::run_filter_tests(None);
    let total = results.outcomes.len();
    let failures: Vec<&toml_filter::TestOutcome> =
        results.outcomes.iter().filter(|o| !o.passed).collect();

    if failures.is_empty() {
        return Diagnostic {
            name: "filter self-tests",
            health: Health::Pass,
            detail: format!("{total}/{total} inline filter tests passed"),
            guidance: None,
            repair: Repair::Manual,
        };
    }

    let passed = total - failures.len();
    let mut detail = format!("{passed}/{total} inline filter tests passed\n");
    for o in &failures {
        detail.push_str(&format!("failing: [{}] {}\n", o.filter_name, o.test_name));
    }

    Diagnostic {
        name: "filter self-tests",
        health: Health::Fail,
        detail: detail.trim_end().to_string(),
        guidance: Some(
            "Filter logic is compiled into this binary, so doctor cannot patch it at \
             runtime.\n  Update TOK to a build with the fix, then re-verify:\n    \
             cargo install --path .   (from a source checkout)\n    # or re-run your \
             install.sh\n    tok verify"
                .to_string(),
        ),
        repair: Repair::Manual,
    }
}

/// Confirm the config file (if any) loads without error.
fn check_config() -> Diagnostic {
    match crate::core::config::Config::load() {
        Ok(_) => Diagnostic {
            name: "configuration",
            health: Health::Pass,
            detail: "config loaded successfully".to_string(),
            guidance: None,
            repair: Repair::Manual,
        },
        Err(e) => Diagnostic {
            name: "configuration",
            health: Health::Warn,
            detail: format!("config failed to load (using defaults): {e}"),
            guidance: Some(
                "Inspect and fix your config with `tok config`. Doctor does not reset \
                 config automatically to avoid discarding your settings."
                    .to_string(),
            ),
            repair: Repair::Manual,
        },
    }
}

/// Reinstall the hook and re-establish its integrity baseline.
///
/// Delegates to `tok init`'s hook-only, auto-patch path — the documented
/// remedy already referenced by the integrity warnings.
fn repair_hook(verbose: u8) -> Result<()> {
    init::run(
        true,            // global
        true,            // install_claude
        false,           // install_opencode
        false,           // install_cursor
        false,           // install_windsurf
        false,           // install_cline
        false,           // claude_md
        true,            // hook_only
        false,           // codex
        PatchMode::Auto, // non-interactive
        verbose,
    )
}

fn collect_diagnostics() -> Vec<Diagnostic> {
    vec![check_hook(), check_filters(), check_config()]
}

/// Entry point for `tok doctor` / `tok doctor --repair`.
///
/// Returns the process exit code: `0` when nothing is broken (warnings are
/// tolerated), `1` when at least one `FAIL` remains after any repairs.
pub fn run(repair: bool, verbose: u8) -> Result<i32> {
    println!("{}", "TOK Doctor".bold());
    println!();

    let mut diagnostics = collect_diagnostics();
    for d in &diagnostics {
        d.print();
    }

    if repair {
        let hook_needs_repair = diagnostics
            .iter()
            .any(|d| d.needs_attention() && d.repair == Repair::Hook);

        println!();
        if hook_needs_repair {
            println!("{}", "Repairing hook…".bold());
            repair_hook(verbose)?;

            println!();
            println!("{}", "Re-checking…".bold());
            println!();
            diagnostics = collect_diagnostics();
            for d in &diagnostics {
                d.print();
            }
        } else {
            println!("Nothing auto-repairable to fix.");
        }
    }

    finish(&diagnostics, repair)
}

/// Print the summary + guidance and compute the exit code.
fn finish(diagnostics: &[Diagnostic], repaired: bool) -> Result<i32> {
    let failed: Vec<&Diagnostic> = diagnostics
        .iter()
        .filter(|d| d.health == Health::Fail)
        .collect();
    let warnings = diagnostics
        .iter()
        .filter(|d| d.health == Health::Warn)
        .count();

    println!();

    if failed.is_empty() && warnings == 0 {
        println!("{}", "All checks passed. TOK is healthy.".green());
        return Ok(0);
    }

    if !failed.is_empty() {
        println!(
            "{}",
            format!("{} check(s) still failing.", failed.len()).red()
        );
        for d in &failed {
            if let Some(guidance) = &d.guidance {
                println!();
                println!("  {}: {}", d.name.bold(), guidance);
            }
        }
        return Ok(1);
    }

    // Warnings only.
    println!(
        "{}",
        format!("{warnings} warning(s); no failures.").yellow()
    );
    if !repaired {
        println!("  Run `tok doctor --repair` to fix repairable issues.");
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_diagnostic_does_not_need_attention() {
        let d = Diagnostic {
            name: "x",
            health: Health::Pass,
            detail: String::new(),
            guidance: None,
            repair: Repair::Hook,
        };
        assert!(!d.needs_attention());
    }

    #[test]
    fn warn_and_fail_need_attention() {
        for health in [Health::Warn, Health::Fail] {
            let d = Diagnostic {
                name: "x",
                health,
                detail: String::new(),
                guidance: None,
                repair: Repair::Hook,
            };
            assert!(d.needs_attention());
        }
    }

    #[test]
    fn all_pass_yields_exit_zero() {
        let diagnostics = vec![Diagnostic {
            name: "x",
            health: Health::Pass,
            detail: String::new(),
            guidance: None,
            repair: Repair::Manual,
        }];
        assert_eq!(finish(&diagnostics, false).unwrap(), 0);
    }

    #[test]
    fn any_fail_yields_exit_one() {
        let diagnostics = vec![
            Diagnostic {
                name: "ok",
                health: Health::Pass,
                detail: String::new(),
                guidance: None,
                repair: Repair::Manual,
            },
            Diagnostic {
                name: "broken",
                health: Health::Fail,
                detail: String::new(),
                guidance: Some("update TOK".to_string()),
                repair: Repair::Manual,
            },
        ];
        assert_eq!(finish(&diagnostics, true).unwrap(), 1);
    }

    #[test]
    fn warnings_only_yield_exit_zero() {
        let diagnostics = vec![Diagnostic {
            name: "x",
            health: Health::Warn,
            detail: String::new(),
            guidance: None,
            repair: Repair::Hook,
        }];
        assert_eq!(finish(&diagnostics, false).unwrap(), 0);
    }

    #[test]
    fn filter_self_tests_pass_on_builtins() {
        // The shipped built-in filters must all pass their inline tests.
        let d = check_filters();
        assert_eq!(
            d.health,
            Health::Pass,
            "built-in filter self-tests should pass: {}",
            d.detail
        );
    }
}
