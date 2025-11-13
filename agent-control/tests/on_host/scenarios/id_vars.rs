#![cfg(target_family = "unix")]
use super::tools::config::{create_file, create_local_config};
use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::retry::retry;
use crate::on_host::consts::NO_CONFIG;
use crate::on_host::tools::custom_agent_type::DYNAMIC_AGENT_TYPE_FILENAME;
use assert_cmd::Command;
use newrelic_agent_control::agent_control::defaults::{
    AGENT_CONTROL_ID, FOLDER_NAME_LOCAL_DATA, STORE_KEY_LOCAL_DATA_CONFIG,
};
use newrelic_agent_control::agent_control::run::BasePaths;
use newrelic_agent_control::on_host::file_store::build_config_name;
use std::time::Duration;
use tempfile::tempdir;

/// tests that nr-ac:host_id and nr-sub:agent_id are correctly replaced in the agent type.
#[test]
fn test_sub_sa_vars() {
    use newrelic_agent_control::agent_control::run::Environment;

    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    create_file(
        r#"
namespace: test
name: test
version: 0.0.0
variables: {}
deployment:
  on_host:
    executables:
      - id: sh
        path: "sh"
        args: >-
          tests/on_host/data/trap_term_sleep_60.sh
          --host_id=${nr-ac:host_id}
          --agent_id=${nr-sub:agent_id}
    "#
        .to_string(),
        local_dir.path().join(DYNAMIC_AGENT_TYPE_FILENAME),
    );
    let sa_config_path = local_dir
        .path()
        .join(FOLDER_NAME_LOCAL_DATA)
        .join(AGENT_CONTROL_ID)
        .join(build_config_name(STORE_KEY_LOCAL_DATA_CONFIG));
    create_file(
        r#"
host_id: fixed-host-id
agents:
  test-agent:
    agent_type: "test/test:0.0.0"
        "#
        .to_string(),
        sa_config_path.clone(),
    );
    create_local_config(
        "test-agent".into(),
        NO_CONFIG.to_string(),
        local_dir.path().into(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };

    let _agent_control = start_agent_control_with_custom_config(base_paths, Environment::OnHost);

    retry(30, Duration::from_secs(1), || {
        // Check that the process is running with this exact command
        Command::new("pgrep")
            .arg("-f")
            .arg("sh tests/on_host/data/trap_term_sleep_60.sh --host_id=fixed-host-id --agent_id=test-agent")
            .assert().try_success()?;
        Ok(())
    });
}
