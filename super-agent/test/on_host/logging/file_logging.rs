use assert_cmd::Command;
use predicates::prelude::predicate;
use std::{fs::read_dir, path::Path, time::Duration};
use tempfile::TempDir;

use crate::logging::level::TIME_FORMAT;

fn build_logging_config(config_path: &Path, log_path: &Path) {
    let config = format!(
        r#"
        log:
            file: 
              enable: true
              path: {}
        "#,
        log_path.to_string_lossy()
    );
    std::fs::write(config_path, config).unwrap();
}

fn cmd_with_config_file(file_path: &Path) -> Command {
    let mut cmd = Command::cargo_bin("newrelic-super-agent").unwrap();
    cmd.arg("--config").arg(file_path);
    // cmd_assert is not made for long running programs, so we kill it anyway after 1 second
    cmd.timeout(Duration::from_secs(1));
    cmd
}

#[test]
fn default_log_level_no_root() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("super_agent.yaml");
    let log_dir = dir.path().join("log");
    let log_path = log_dir.join("super_agent.log");

    // Write the config file
    build_logging_config(&config_path, &log_path);

    let mut cmd = cmd_with_config_file(&config_path);

    // Expecting to fail as non_root
    // Asserting content is logged to stdout as well
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

    // The behavior of the appender functionality is already unit tested as part of the sub-agent
    // logging feature. Here we just assert that the files are created.
    let dir: Vec<_> = read_dir(log_dir)
        .unwrap()
        // We unwrap each entry to be able to inspect it
        .map(|entry| entry.unwrap())
        .collect();

    for file in dir {
        assert!(file
            .path()
            .to_str()
            .unwrap()
            .contains(log_path.to_str().unwrap()));
    }
}

#[test]
fn default_log_level_as_root() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("super_agent.yaml");
    let log_dir = dir.path().join("log");
    let log_path = log_dir.join("super_agent.log");

    // Write the config file
    build_logging_config(&config_path, &log_path);

    let mut cmd = cmd_with_config_file(&config_path);

    // Expecting to fail as non_root
    // Asserting content is logged to stdout as well
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
                TIME_FORMAT.to_owned() + "INFO.*Starting the agents supervisor runtime",
            )
                .unwrap(),
        );

    // The behavior of the appender functionality is already unit tested as part of the sub-agent
    // logging feature. Here we just assert that the files are created.
    let dir: Vec<_> = read_dir(log_dir)
        .unwrap()
        // We unwrap each entry to be able to inspect it
        .map(|entry| entry.unwrap())
        .collect();

    for file in dir {
        assert!(file
            .path()
            .to_str()
            .unwrap()
            .contains(log_path.to_str().unwrap()));
    }
}
