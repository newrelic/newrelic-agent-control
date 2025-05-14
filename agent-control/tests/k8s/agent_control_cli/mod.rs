use assert_cmd::Command;
use predicates::prelude::*;

mod dynamic_objects;
mod installation;

#[test]
fn cli_install_agent_control_fails_when_no_kubernetes() {
    let mut cmd = Command::cargo_bin("newrelic-agent-control-cli").unwrap();
    cmd.arg("install").arg("agent-control");
    cmd.arg("--release-name").arg("agent-control-release");
    cmd.arg("--chart-version").arg("1.0.0");

    cmd.assert().failure();
    cmd.assert().code(predicate::eq(69));
}
