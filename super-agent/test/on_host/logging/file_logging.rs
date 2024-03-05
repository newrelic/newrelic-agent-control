use assert_cmd::Command;
use predicates::prelude::predicate;
use std::{fs::read_dir, path::Path, time::Duration};
use tempfile::TempDir;

use crate::logging::level::TIME_FORMAT;

#[cfg(test)]
fn build_logging_config(config_path: &Path, log_path: &Path) {
    let config = format!(
        r#"
        agents: {{}}
        log:
            file: 
              enable: true
              path: {}
        "#,
        log_path.to_string_lossy()
    );
    std::fs::write(config_path, config).unwrap();
}

#[cfg(test)]
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

    // Let's wait for a second so the flushed contents arrive to the files
    std::thread::sleep(Duration::from_secs(2));

    // Now, we assert that the file(s) created are present and have the expected content
    let dir: Vec<_> = read_dir(log_dir)
        .unwrap()
        // We unwrap each entry to be able to order it
        .map(|entry| entry.unwrap())
        .collect();
    // if sorting is needed, use
    // dir.sort_by_key(|f| f.path());

    // We append the contents of the files in order
    let mut actual = String::new();
    for file in dir {
        actual.push_str(&std::fs::read_to_string(file.path()).unwrap());
    }

    assert!(actual.contains("ERROR Program must run as root"));
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

    // Let's wait for a second so the flushed contents arrive to the files
    std::thread::sleep(Duration::from_secs(1));

    // Now, we assert that the file(s) created are present and have the expected content
    let dir: Vec<_> = read_dir(log_dir)
        .unwrap()
        // We unwrap each entry to be able to order it
        .map(|entry| entry.unwrap())
        .collect();
    // if sorting is needed, use
    // dir.sort_by_key(|f| f.path());

    // We append the contents of the files in order
    let mut actual = String::new();
    for file in dir {
        actual.push_str(&std::fs::read_to_string(file.path()).unwrap());
    }

    assert!(actual.contains("INFO Instance Identifiers:"));
    assert!(actual.contains("INFO Starting NewRelic Super Agent"));
    assert!(actual.contains("INFO Starting the agents supervisor runtime"));
}
