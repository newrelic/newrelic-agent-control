// Given Agent Control runs with a defined sub-agent and then Agent Control stops,
// there should be no orphan processes left behind.

use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::opamp::FakeServer;
use crate::common::process_finder::find_processes_by_pattern;
use crate::on_host::tools::config::{create_agent_control_config, create_local_config};
use crate::on_host::tools::custom_agent_type::CustomAgentType;
use newrelic_agent_control::agent_control::run::{BasePaths, Environment};
use std::thread;
use std::time::Duration;
use tempfile::tempdir;

/// Test that verifies no orphan processes are left when Agent Control stops.
/// This test works on both Unix and Windows platforms.
#[test]
fn test_no_orphan_processes_after_agent_control_stops() {
    let opamp_server = FakeServer::start_new();

    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    // Create a custom agent type with a long-running sleep process
    #[cfg(target_family = "unix")]
    let agent_type = CustomAgentType::empty()
        .with_executables(Some(
            r#"[
                {"id": "long-sleep", "path": "sleep", "args": "3600"}
            ]"#,
        ))
        .build(local_dir.path().to_path_buf());

    #[cfg(target_family = "windows")]
    let agent_type = CustomAgentType::empty()
        .with_executables(Some(
            r#"[
                {"id": "long-sleep", "path": "powershell", "args": "-Command Start-Sleep -Seconds 3600"}
            ]"#,
        ))
        .build(local_dir.path().to_path_buf());

    let agents = format!(
        r#"
agents:
  test-agent:
    agent_type: "{agent_type}"
"#
    );

    create_agent_control_config(
        opamp_server.endpoint(),
        opamp_server.jwks_endpoint(),
        agents,
        local_dir.path().to_path_buf(),
    );

    // Create local config to trigger the executable launch
    create_local_config(
        "test-agent".to_string(),
        "fake_variable: value".to_string(),
        local_dir.path().to_path_buf(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };

    // Start Agent Control
    let agent_control = start_agent_control_with_custom_config(base_paths, Environment::OnHost);

    // Wait for processes to start
    thread::sleep(Duration::from_secs(10));

    // Find the spawned sub-agent process
    #[cfg(target_family = "unix")]
    let process_pattern = "sleep 3600";
    #[cfg(target_family = "windows")]
    let process_pattern = "Start-Sleep -Seconds 3600";

    let pids_before = find_processes_by_pattern(process_pattern);
    assert!(
        !pids_before.is_empty(),
        "Sub-agent process should be running before stopping Agent Control"
    );
    assert!(
        pids_before.len() == 1,
        "Expected exactly one sub-agent process, found: {:?}",
        pids_before
    );

    // Stop Agent Control by dropping it
    drop(agent_control);

    // Give some time for cleanup
    thread::sleep(Duration::from_secs(5));

    // Check that the processes are no longer running
    let pids_after = find_processes_by_pattern(process_pattern);

    assert!(
        pids_after.is_empty(),
        "Expected no orphan processes after Agent Control stops, but found: {:?}",
        pids_after
    );
}
