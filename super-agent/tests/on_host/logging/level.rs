use predicates::prelude::predicate;
use tempfile::TempDir;

use crate::on_host::cli::cmd_with_config_file;

const EMPTY_CONFIG: &str = "# Empty config\nagents: {}";

const DEBUG_LEVEL_CONFIG: &str = "agents: {}\nlog:\n  level: debug";

const TRACE_LEVEL_CONFIG: &str = "agents: {}\nlog:\n  level: trace";

pub(crate) const TIME_FORMAT: &str = r".*(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}).*";

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
            predicate::str::is_match(TIME_FORMAT.to_owned() + "ERROR.*Program must run as root")
                .unwrap(),
        );
}

#[test]
fn trace_log_level_as_root() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("super_agent.yaml");
    std::fs::write(&config_path, TRACE_LEVEL_CONFIG).unwrap();

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
            predicate::str::is_match(
                TIME_FORMAT.to_owned() + "INFO.*Starting NewRelic Super Agent",
            )
            .unwrap(),
        )
        .stdout(
            predicate::str::is_match(TIME_FORMAT.to_owned() + "TRACE.*creating the signal handler")
                .unwrap(),
        )
        .stdout(
            predicate::str::is_match(TIME_FORMAT.to_owned() + "TRACE.*creating the global context")
                .unwrap(),
        )
        .stdout(
            predicate::str::is_match(
                TIME_FORMAT.to_owned() + "INFO.*Starting the agents supervisor runtime",
            )
            .unwrap(),
        );
}
