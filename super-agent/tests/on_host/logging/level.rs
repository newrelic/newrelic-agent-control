use assert_cmd::Command;
use predicates::prelude::predicate;
use std::{path::Path, time::Duration};
use tempfile::TempDir;

#[cfg(test)]
const EMPTY_CONFIG: &str = "# Empty config\nagents: {}";
#[cfg(test)]
const DEBUG_LEVEL_CONFIG: &str = "agents: {}\nlog:\n  level: debug";
#[cfg(test)]
pub(crate) const TIME_FORMAT: &str = r".*(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}).*";

#[cfg(test)]
fn cmd_with_config_file(file_path: &Path) -> Command {
    let mut cmd = Command::cargo_bin("newrelic-super-agent").unwrap();
    cmd.arg("--config").arg(file_path);
    // cmd_assert is not made for long running programs, so we kill it anyway after 1 second
    cmd.timeout(Duration::from_secs(2));
    cmd
}

#[test]
fn default_log_level_no_root() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("super_agent.yaml");
    std::fs::write(&config_path, EMPTY_CONFIG).unwrap();

    let mut cmd = cmd_with_config_file(&config_path);
    // Expecting to fail as non_root
    cmd.assert()
        .failure()
        .stdout(
            predicate::str::is_match(
                TIME_FORMAT.to_owned() + "INFO.*New Relic Super Agent Version: .*, Rust Version: .*, GitCommit: .*, BuildDate: .*",
            )
                .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                TIME_FORMAT.to_owned() + "ERROR.*Program must run as root",
            )
                .unwrap(),
        );
}

#[test]
fn default_log_level_as_root() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("super_agent.yaml");
    std::fs::write(&config_path, EMPTY_CONFIG).unwrap();

    let mut cmd = cmd_with_config_file(&config_path);
    // Expecting to fail as non_root
    cmd.assert()
        .failure()
        .stdout(
            predicate::str::is_match(
                TIME_FORMAT.to_owned() + "INFO.*New Relic Super Agent Version: .*, Rust Version: .*, GitCommit: .*, BuildDate: .*",
            )
                .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                TIME_FORMAT.to_owned() + "INFO.*Starting NewRelic Super Agent",
            )
                .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                TIME_FORMAT.to_owned() + "INFO.*Starting the agents supervisor runtime",
            )
                .unwrap(),
        );
}

#[test]
fn debug_log_level_no_root() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("super_agent.yaml");
    std::fs::write(&config_path, DEBUG_LEVEL_CONFIG).unwrap();

    let mut cmd = cmd_with_config_file(&config_path);

    // Expecting to fail as non_root
    cmd.assert()
        .failure()
        .stdout(
            predicate::str::is_match(
                TIME_FORMAT.to_owned() + "DEBUG.*Logging initialized successfully",
            )
            .unwrap(),
        )
        .stdout(
            predicate::str::is_match(TIME_FORMAT.to_owned() + "DEBUG.*Creating the signal handler")
                .unwrap(),
        )
        .stdout(
            predicate::str::is_match(TIME_FORMAT.to_owned() + "DEBUG.*Creating the global context")
                .unwrap(),
        )
        .stdout(
            predicate::str::is_match(TIME_FORMAT.to_owned() + "ERROR.*Program must run as root")
                .unwrap(),
        );
}

#[test]
fn debug_log_level_as_root() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("super_agent.yaml");
    std::fs::write(&config_path, DEBUG_LEVEL_CONFIG).unwrap();

    let mut cmd = cmd_with_config_file(&config_path);

    // Expecting to fail as non_root
    cmd.assert()
        .failure()
        .stdout(
            predicate::str::is_match(
                TIME_FORMAT.to_owned() + "DEBUG.*Logging initialized successfully",
            )
            .unwrap(),
        )
        .stdout(
            predicate::str::is_match(TIME_FORMAT.to_owned() + "DEBUG.*Creating the signal handler")
                .unwrap(),
        )
        .stdout(
            predicate::str::is_match(TIME_FORMAT.to_owned() + "DEBUG.*Creating the global context")
                .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                TIME_FORMAT.to_owned() + "INFO.*Starting NewRelic Super Agent",
            )
            .unwrap(),
        )
        .stdout(
            predicate::str::is_match(TIME_FORMAT.to_owned() + "DEBUG.*Creating the signal handler")
                .unwrap(),
        )
        .stdout(
            predicate::str::is_match(TIME_FORMAT.to_owned() + "DEBUG.*Creating the global context")
                .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                TIME_FORMAT.to_owned() + "INFO.*Starting the agents supervisor runtime",
            )
            .unwrap(),
        );
}
