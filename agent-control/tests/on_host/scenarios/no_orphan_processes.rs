// Given Agent Control runs with a defined sub-agent and then Agent Control stops,
// there should be no orphan processes left behind.

use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::opamp::FakeServer;
use crate::common::process_finder::find_processes_by_pattern;
use crate::common::retry::retry;
use crate::on_host::cli::create_temp_file;
use crate::on_host::tools::config::{create_agent_control_config, create_local_config};
use crate::on_host::tools::custom_agent_type::CustomAgentType;
use assert_cmd::cargo::cargo_bin_cmd;
use newrelic_agent_control::agent_control::defaults::{
    AGENT_CONTROL_ID, DYNAMIC_AGENT_TYPE_DIR, FOLDER_NAME_LOCAL_DATA, STORE_KEY_LOCAL_DATA_CONFIG,
};
use newrelic_agent_control::agent_control::run::BasePaths;
use newrelic_agent_control::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;
use newrelic_agent_control::on_host::file_store::build_config_name;
use std::thread;
use std::time::Duration;
use tempfile::{TempDir, tempdir};

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
    let agent_control =
        start_agent_control_with_custom_config(base_paths, AGENT_CONTROL_MODE_ON_HOST);

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

/// Test that verifies no orphan processes are left when the Agent Control binary is killed.
/// This test spawns the actual binary using assert_cmd and validates cleanup on termination.
#[test]
#[ignore = "requires root"]
fn test_no_orphan_processes_when_binary_killed_as_root() {
    let local_dir = TempDir::new().unwrap();

    // Create agent type YAML with platform-specific sleep commands
    #[cfg(target_family = "unix")]
    let agent_type_yaml = r#"
namespace: newrelic
name: com.newrelic.test-agent
version: 0.0.1
variables:
  on_host:
    duration:
      description: "time to sleep"
      type: string
      required: false
      default: "3600"
deployment:
  on_host:
    enable_file_logging: false
    executables:
      - id: long-sleep
        path: sleep
        args: "${nr-var:duration}"
"#;

    #[cfg(target_family = "windows")]
    let agent_type_yaml = r#"
namespace: newrelic
name: com.newrelic.test-agent
version: 0.0.1
variables:
  on_host:
    duration:
      description: "time to sleep"
      type: string
      required: false
      default: "3600"
deployment:
  on_host:
    enable_file_logging: false
    executables:
      - id: long-sleep
        path: powershell
        args: "-Command Start-Sleep -Seconds ${nr-var:duration}"
"#;

    let _agent_type_def = create_temp_file(
        local_dir.path().join(DYNAMIC_AGENT_TYPE_DIR).as_path(),
        "test-agent.yaml",
        agent_type_yaml,
    )
    .unwrap();

    let _values_file = create_temp_file(
        local_dir
            .path()
            .join(FOLDER_NAME_LOCAL_DATA)
            .join("test-agent")
            .as_path(),
        build_config_name(STORE_KEY_LOCAL_DATA_CONFIG).as_str(),
        r#"duration: "12345""#,
    )
    .unwrap();

    let _config_path = create_temp_file(
        &local_dir
            .path()
            .join(FOLDER_NAME_LOCAL_DATA)
            .join(AGENT_CONTROL_ID),
        build_config_name(STORE_KEY_LOCAL_DATA_CONFIG).as_str(),
        r#"
log:
  level: debug
  file:
    enabled: true
agents:
  test-agent:
    agent_type: newrelic/com.newrelic.test-agent:0.0.1
server:
  enabled: false
"#,
    )
    .unwrap();

    // Spawn agent control in a background thread
    let local_dir_path = local_dir.path().to_path_buf();
    let agent_control_handle = thread::spawn(move || {
        let mut cmd = cargo_bin_cmd!("newrelic-agent-control");
        cmd.arg("--local-dir").arg(local_dir_path);
        // Run for a limited time. Using `timeout` from `assert_cmd`, if
        // the process does not complete before the timeout then it will be terminated
        // using [std::process::Child::kill], which on Unix sends a `SIGKILL`.
        cmd.timeout(Duration::from_secs(60));
        cmd.assert()
    });

    // Wait for the process to start and find the spawned sub-agent
    #[cfg(target_family = "unix")]
    let process_pattern = "sleep 12345";
    #[cfg(target_family = "windows")]
    let process_pattern = "Start-Sleep -Seconds 12345";

    // Give time for AC setup
    thread::sleep(Duration::from_secs(10));
    let _sleep_pid = retry(30, Duration::from_secs(1), || {
        let pids = find_processes_by_pattern(process_pattern);
        if pids.is_empty() {
            Err("Sub-agent sleep process not found yet".into())
        } else {
            Ok(pids[0].clone())
        }
    });

    // Wait for the spawned thread to complete
    let ac_process_assert = agent_control_handle.join().unwrap();
    // Assert that the process was indeed interrupted (killed)
    ac_process_assert.interrupted();
    // Give some more time for cleanup
    thread::sleep(Duration::from_secs(10));

    // Verify the sub-agent process was also terminated
    retry(30, Duration::from_secs(1), || {
        let pids = find_processes_by_pattern(process_pattern);
        if pids.is_empty() {
            Ok(())
        } else {
            Err(format!("Orphan sub-agent processes still running. PIDs {pids:?}").into())
        }
    });
}
