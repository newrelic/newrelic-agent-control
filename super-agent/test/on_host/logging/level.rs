use assert_cmd::Command;
use predicates::prelude::predicate;
use std::{path::Path, time::Duration};

const EMPTY_CONFIG_FILE: &str = "test/on_host/logging/configs/empty_config.yaml";
const DEBUG_LEVEL_FILE: &str = "test/on_host/logging/configs/debug_level.yaml";

fn cmd_with_config_file(file_path: &Path) -> Command {
    let mut cmd = Command::cargo_bin("newrelic-super-agent").unwrap();
    cmd.arg("--config").arg(file_path);
    // cmd_assert is not made for long running programs, so we kill it anyway after 1 second
    cmd.timeout(Duration::from_secs(1));
    cmd
}

#[test]
fn default_log_level_no_root() {
    let mut cmd = cmd_with_config_file(Path::new(EMPTY_CONFIG_FILE));
    // Expecting to fail as non_root
    cmd.assert()
        .failure()
        .stdout(
            predicate::str::is_match(
                r".*(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}).*INFO.*Creating the signal handler",
            )
            .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                r".*(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}).*INFO.*Creating the global context",
            )
            .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                r".*(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}).*ERROR.*Program must run as root",
            )
            .unwrap(),
        );
}
#[test]
fn default_log_level_as_root() {
    let mut cmd = cmd_with_config_file(Path::new(EMPTY_CONFIG_FILE));
    // Expecting to fail as non_root
    cmd.assert()
        .failure()
        .stdout(
            predicate::str::is_match(
                r".*(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}).*INFO.*Creating the signal handler",
            )
            .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                r".*(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}).*INFO.*Creating the global context",
            )
            .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                r".*(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}).*INFO.*Starting the super agent",
            )
            .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                r".*(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}).*INFO.*Starting the supervisor group",
            )
            .unwrap(),
        );
}

#[test]
fn debug_log_level_no_root() {
    let mut cmd = cmd_with_config_file(Path::new(DEBUG_LEVEL_FILE));
    // Expecting to fail as non_root
    cmd.assert()
        .failure()
        .stdout(
            predicate::str::is_match(
                r".*(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}).*DEBUG.*Logging initialized successfully",
            )
            .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                r".*(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}).*INFO.*Creating the signal handler",
            )
            .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                r".*(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}).*INFO.*Creating the global context",
            )
            .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                r".*(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}).*ERROR.*Program must run as root",
            )
            .unwrap(),
        );
}

#[test]
fn debug_log_level_as_root() {
    let mut cmd = cmd_with_config_file(Path::new(DEBUG_LEVEL_FILE));
    // Expecting to fail as non_root
    cmd.assert()
        .failure()
        .stdout(
            predicate::str::is_match(
                r".*(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}).*DEBUG.*Logging initialized successfully",
            )
            .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                r".*(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}).*INFO.*Creating the signal handler",
            )
            .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                r".*(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}).*INFO.*Creating the global context",
            )
            .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                r".*(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}).*INFO.*Starting the super agent",
            )
            .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                r".*(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}).*INFO.*Starting the supervisor group",
            )
            .unwrap(),
        );
}
