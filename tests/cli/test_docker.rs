use predicates::prelude::*;

use super::tok_cmd;

#[test]
fn docker_help() {
    skip_if_missing!("docker");
    tok_cmd()
        .args(["docker", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}

#[test]
fn kubectl_help() {
    skip_if_missing!("kubectl");
    tok_cmd()
        .args(["kubectl", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}

#[test]
fn docker_ps() {
    skip_if_docker_unavailable!();
    tok_cmd().args(["docker", "ps"]).assert().success();
}

#[test]
fn docker_images() {
    skip_if_docker_unavailable!();
    tok_cmd().args(["docker", "images"]).assert().success();
}
