//! TOK CLI Integration Test Harness
//!
//! Run with: cargo test --test cli

use assert_cmd::Command;

/// Build a `Command` for the `tok` binary under test.
fn tok_cmd() -> Command {
    Command::cargo_bin("tok").expect("tok binary not found — run `cargo build` first")
}

/// Skip a test when an external tool is not installed.
macro_rules! skip_if_missing {
    ($tool:expr) => {
        if which::which($tool).is_err() {
            eprintln!("SKIP: {} not found on PATH", $tool);
            return;
        }
    };
}

/// Skip a test when `gh` is not authenticated.
macro_rules! skip_if_gh_unauthed {
    () => {
        skip_if_missing!("gh");
        let auth = std::process::Command::new("gh")
            .args(["auth", "status"])
            .output();
        match auth {
            Ok(o) if o.status.success() => {}
            _ => {
                eprintln!("SKIP: gh not authenticated");
                return;
            }
        }
    };
}

/// Skip a test when Docker daemon is not running.
macro_rules! skip_if_docker_unavailable {
    () => {
        skip_if_missing!("docker");
        let info = std::process::Command::new("docker")
            .arg("info")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        match info {
            Ok(s) if s.success() => {}
            _ => {
                eprintln!("SKIP: docker daemon not running");
                return;
            }
        }
    };
}

#[path = "cli/test_aws.rs"]
mod test_aws;
#[path = "cli/test_cargo.rs"]
mod test_cargo;
#[path = "cli/test_cc_economics.rs"]
mod test_cc_economics;
#[path = "cli/test_config.rs"]
mod test_config;
#[path = "cli/test_curl.rs"]
mod test_curl;
#[path = "cli/test_deps.rs"]
mod test_deps;
#[path = "cli/test_diff.rs"]
mod test_diff;
#[path = "cli/test_discover.rs"]
mod test_discover;
#[path = "cli/test_docker.rs"]
mod test_docker;
#[path = "cli/test_dotnet.rs"]
mod test_dotnet;
#[path = "cli/test_env.rs"]
mod test_env;
#[path = "cli/test_err.rs"]
mod test_err;
#[path = "cli/test_find.rs"]
mod test_find;
#[path = "cli/test_format.rs"]
mod test_format;
#[path = "cli/test_gain.rs"]
mod test_gain;
#[path = "cli/test_gh.rs"]
mod test_gh;
#[path = "cli/test_git.rs"]
mod test_git;
#[path = "cli/test_global_flags.rs"]
mod test_global_flags;
#[path = "cli/test_go.rs"]
mod test_go;
#[path = "cli/test_graphite.rs"]
mod test_graphite;
#[path = "cli/test_grep.rs"]
mod test_grep;
#[path = "cli/test_hook.rs"]
mod test_hook;
#[path = "cli/test_hook_audit.rs"]
mod test_hook_audit;
#[path = "cli/test_init.rs"]
mod test_init;
#[path = "cli/test_js_tools.rs"]
mod test_js_tools;
#[path = "cli/test_json.rs"]
mod test_json;
#[path = "cli/test_learn.rs"]
mod test_learn;
#[path = "cli/test_log.rs"]
mod test_log;
#[path = "cli/test_ls.rs"]
mod test_ls;
#[path = "cli/test_npm.rs"]
mod test_npm;
#[path = "cli/test_pnpm.rs"]
mod test_pnpm;
#[path = "cli/test_prisma.rs"]
mod test_prisma;
#[path = "cli/test_proxy.rs"]
mod test_proxy;
#[path = "cli/test_psql.rs"]
mod test_psql;
#[path = "cli/test_python.rs"]
mod test_python;
#[path = "cli/test_read.rs"]
mod test_read;
#[path = "cli/test_rewrite.rs"]
mod test_rewrite;
#[path = "cli/test_ruby.rs"]
mod test_ruby;
#[path = "cli/test_security.rs"]
mod test_security;
#[path = "cli/test_session.rs"]
mod test_session;
#[path = "cli/test_smart.rs"]
mod test_smart;
#[path = "cli/test_summary.rs"]
mod test_summary;
#[path = "cli/test_test_runner.rs"]
mod test_test_runner;
#[path = "cli/test_tree.rs"]
mod test_tree;
#[path = "cli/test_trust.rs"]
mod test_trust;
#[path = "cli/test_update.rs"]
mod test_update;
#[path = "cli/test_verify.rs"]
mod test_verify;
#[path = "cli/test_version_help.rs"]
mod test_version_help;
#[path = "cli/test_vitest.rs"]
mod test_vitest;
#[path = "cli/test_wc.rs"]
mod test_wc;
#[path = "cli/test_wget.rs"]
mod test_wget;
