use assert_cmd::Command;
use predicates::prelude::*;

mod helm_release;
mod helm_repository;

#[test]
fn cli_fails_when_no_kubernetes() {
    let mut cmd = Command::cargo_bin("newrelic-agent-control-cli").unwrap();
    cmd.arg("create").arg("helm-repository");
    cmd.arg("--name").arg("name");
    cmd.arg("--url").arg("url");
    cmd.arg("--namespace").arg("namespace");
    cmd.assert().failure();
    cmd.assert().code(predicate::eq(69));
}
