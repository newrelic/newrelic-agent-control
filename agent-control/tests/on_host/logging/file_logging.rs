#![cfg(target_family = "unix")]
use super::level::TIME_FORMAT;
use crate::on_host::cli::cmd_with_config_file;
use newrelic_agent_control::{
    agent_control::defaults::{
        AGENT_CONTROL_ID, FOLDER_NAME_LOCAL_DATA, STORE_KEY_LOCAL_DATA_CONFIG,
    },
    on_host::file_store::build_config_name,
};
use predicates::prelude::predicate;
use std::{fs::read_dir, path::Path};
use tempfile::TempDir;

fn build_logging_config(config_path: &Path, log_path: &Path) {
    let config = format!(
        r#"
        agents: {{}}
        log:
            file:
              enabled: true
              path: {}
        "#,
        log_path.to_string_lossy()
    );
    std::fs::write(config_path, config).unwrap();
}

#[cfg(all(target_family = "unix", not(feature = "disable-asroot")))]
#[test]
fn default_log_level_no_root() {
    let dir = TempDir::new().unwrap();
    let config_path = dir
        .path()
        .join(FOLDER_NAME_LOCAL_DATA)
        .join(AGENT_CONTROL_ID);
    std::fs::create_dir_all(&config_path).unwrap();
    let log_dir = dir.path().join("log");
    let log_path = log_dir.join("agent_control.log");

    // Write the config file
    build_logging_config(
        &config_path.join(build_config_name(STORE_KEY_LOCAL_DATA_CONFIG)),
        &log_path,
    );

    let mut cmd = cmd_with_config_file(dir.path());

    // Expecting to fail as non_root
    // Asserting content is logged to stdout as well
    cmd.assert().failure().stdout(
        predicate::str::is_match(
            TIME_FORMAT.to_owned() + "ERROR.*Program must run with elevated permissions",
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
        assert!(
            file.path()
                .to_str()
                .unwrap()
                .contains(log_path.to_str().unwrap())
        );
    }
}

#[test]
#[ignore = "requires root"]
fn default_log_level_as_root() {
    let dir = TempDir::new().unwrap();
    let config_path = dir
        .path()
        .join(FOLDER_NAME_LOCAL_DATA)
        .join(AGENT_CONTROL_ID);
    std::fs::create_dir_all(&config_path).unwrap();
    let log_dir = dir.path().join("log");
    let log_path = log_dir.join("agent_control.log");

    // Write the config file
    build_logging_config(
        &config_path.join(build_config_name(STORE_KEY_LOCAL_DATA_CONFIG)),
        &log_path,
    );

    let mut cmd = cmd_with_config_file(dir.path());

    // Expecting to fail as non_root
    // Asserting content is logged to stdout as well
    cmd.assert()
        .failure()
        .stdout(
            predicate::str::is_match(
                TIME_FORMAT.to_owned()
                    + "INFO.*New Relic Agent Control Version: .*, Rust Version: .*, GitCommit: .*",
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
        assert!(
            file.path()
                .to_str()
                .unwrap()
                .contains(log_path.to_str().unwrap())
        );
    }
}
