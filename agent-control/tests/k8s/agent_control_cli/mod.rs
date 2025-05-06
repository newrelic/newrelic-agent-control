use assert_cmd::Command;
use predicates::prelude::*;

mod install_agent_control;

#[test]
fn cli_fails_when_no_kubernetes() {
    let mut cmd = Command::cargo_bin("newrelic-agent-control-cli").unwrap();
    cmd.arg("install").arg("agent-control");
    cmd.arg("--release-name").arg("agent-control-release");
    cmd.arg("--chart-version").arg("1.0.0");
    cmd.assert().failure();
    cmd.assert().code(predicate::eq(69));
}
