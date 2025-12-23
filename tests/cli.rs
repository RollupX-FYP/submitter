use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn test_submitter_help() {
    let mut cmd = Command::cargo_bin("submitter").unwrap();
    cmd.arg("--help").assert().success();
}

#[test]
fn test_submitter_rs_help() {
    let mut cmd = Command::cargo_bin("submitter-rs").unwrap();
    cmd.arg("--help").assert().success();
}

#[test]
fn test_submitter_missing_config() {
    let mut cmd = Command::cargo_bin("submitter").unwrap();
    cmd.assert().failure().stderr(predicate::str::contains("Usage:"));
}
