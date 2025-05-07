use assert_cmd::Command;
use predicates::prelude::*;

use rstest::rstest;

#[rstest]
#[case::no_kubernetes(vec![], 69)]
#[case::invalid_values(vec![("--values", "key1: value1\nkey2 value2")], 65)]
#[case::values_files_does_not_exist(vec![("--values", "fs://nonexistent.yaml")], 66)]
fn cli_install_agent_control_fails(#[case] args: Vec<(&str, &str)>, #[case] expected_code: i32) {
    let mut cmd = Command::cargo_bin("newrelic-agent-control-cli").unwrap();
    cmd.arg("install").arg("agent-control");
    cmd.arg("--release-name").arg("agent-control-release");
    cmd.arg("--chart-version").arg("1.0.0");

    for (key, value) in args {
        cmd.arg(key).arg(value);
    }

    cmd.assert().failure();
    cmd.assert().code(predicate::eq(expected_code));
}
