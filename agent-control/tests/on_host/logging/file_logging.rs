#![cfg(target_family = "unix")]
use std::fs::read_dir;

use super::level::TIME_FORMAT;
use crate::on_host::{cli::cmd_with_config_file, tools::config::create_file};
use newrelic_agent_control::agent_control::defaults::{
    AGENT_CONTROL_ID, AGENT_CONTROL_LOG_FILENAME, FOLDER_NAME_LOCAL_DATA,
    STORE_KEY_LOCAL_DATA_CONFIG,
};
use predicates::prelude::predicate;
use tempfile::TempDir;

#[test]
#[ignore = "requires root"]
fn test_ac_log_to_file_as_root() {
    let local_dir = TempDir::new().unwrap();

    let config_path = local_dir
        .path()
        .join(FOLDER_NAME_LOCAL_DATA)
        .join(AGENT_CONTROL_ID)
        .join(STORE_KEY_LOCAL_DATA_CONFIG.to_string() + ".yaml");

    let log_file_path = local_dir.path().join(AGENT_CONTROL_LOG_FILENAME);

    let config = format!(
        r#"
    host_id: integration-test
    agents: {{}}
    log:
      file:
        enabled: true
        path: {}
    "#,
        log_file_path.to_str().unwrap()
    );

    create_file(config, config_path);

    let mut cmd = cmd_with_config_file(local_dir.path());

    let log_predicate = predicate::str::is_match(
        TIME_FORMAT.to_owned() + "INFO.*Agents supervisor runtime successfully started",
    )
    .unwrap();
    dbg!(&log_file_path);
    // Asserting content is logged to stdout as well
    // The failure is just because of the timeout set in the command execution.
    cmd.assert().failure().stdout(log_predicate.clone());

    // The behavior of the appender functionality is already unit tested as part of the sub-agent
    // logging feature. Here we just assert that the files are created.
    let dir: Vec<String> = read_dir(local_dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().path().to_str().unwrap().to_string())
        .collect();

    dir.iter()
        .find(|path| path.contains(AGENT_CONTROL_LOG_FILENAME))
        .expect("Log file not found in the local directory");
}
