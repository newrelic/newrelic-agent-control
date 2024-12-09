use crate::on_host::cli::cmd_with_config_file;
use newrelic_super_agent::super_agent::defaults::SUPER_AGENT_CONFIG_FILE;
use predicates::prelude::predicate;
use tempfile::TempDir;

const EMPTY_CONFIG: &str = "# Empty config\nagents: {}";

const DEBUG_LEVEL_CONFIG: &str = "agents: {}\nlog:\n  level: debug";

const TRACE_LEVEL_CONFIG: &str = "agents: {}\nlog:\n  level: trace";

pub(crate) const TIME_FORMAT: &str = r".*(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}).*";

#[test]
fn default_log_level_no_root() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join(SUPER_AGENT_CONFIG_FILE), EMPTY_CONFIG).unwrap();

    let mut cmd = cmd_with_config_file(dir.path());
    // Expecting to fail as non_root
    cmd.assert()
        .failure()
        .stdout(
            predicate::str::is_match(
                TIME_FORMAT.to_owned()
                    + "INFO.*New Relic Super Agent Version: .*, Rust Version: .*, GitCommit: .*",
            )
            .unwrap(),
        )
        .stdout(
            predicate::str::is_match(TIME_FORMAT.to_owned() + "ERROR.*Program must run as root")
                .unwrap(),
        );
}

#[test]
fn default_log_level_as_root() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join(SUPER_AGENT_CONFIG_FILE), EMPTY_CONFIG).unwrap();

    let mut cmd = cmd_with_config_file(dir.path());
    cmd.assert()
        .failure()
        .stdout(
            predicate::str::is_match(
                TIME_FORMAT.to_owned()
                    + "INFO.*New Relic Super Agent Version: .*, Rust Version: .*, GitCommit: .*",
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
    std::fs::write(dir.path().join(SUPER_AGENT_CONFIG_FILE), DEBUG_LEVEL_CONFIG).unwrap();

    let mut cmd = cmd_with_config_file(dir.path());
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
    std::fs::write(dir.path().join(SUPER_AGENT_CONFIG_FILE), TRACE_LEVEL_CONFIG).unwrap();

    let mut cmd = cmd_with_config_file(dir.path());
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
