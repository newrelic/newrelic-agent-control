// Given Agent Control runs with a defined sub-agent and then Agent Control stops,
// there should be no orphan processes left behind.

use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::process_finder::find_processes_by_pattern;
use crate::common::runtime::tokio_runtime;
use crate::on_host::tools::base_paths::TempBasePaths;
use crate::on_host::tools::config::{AgentControlConfigBuilder, create_local_config};
use crate::on_host::tools::custom_agent_type::CustomAgentType;
use fake_opamp_server::FakeServer;
use newrelic_agent_control::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;
use std::thread;
use std::time::Duration;

/// Test that verifies no orphan processes are left when Agent Control stops.
/// This test works on both Unix and Windows platforms.
#[test]
fn test_no_orphan_processes_after_agent_control_stops() {
    let opamp_server = FakeServer::start(tokio_runtime().handle());

    let dirs = TempBasePaths::new();

    // Create a custom agent type with a long-running sleep process
    #[cfg(target_family = "unix")]
    let agent_type = CustomAgentType::empty()
        .with_executables(Some(
            r#"[
                {"id": "long-sleep", "path": "sleep", "args": ["3600"]}
            ]"#,
        ))
        .build(dirs.local_dir());

    #[cfg(target_family = "windows")]
    let agent_type = CustomAgentType::empty()
        .with_executables(Some(
            r#"[
                {"id": "long-sleep", "path": "powershell", "args": ["-Command","Start-Sleep","-Seconds","3600"]}
            ]"#,
        ))
        .build(dirs.local_dir());

    let agents = format!(
        r#"
agents:
  test-agent:
    agent_type: "{agent_type}"
"#
    );

    AgentControlConfigBuilder::basic(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_agents(agents)
        .write(dirs.local_dir());

    // Create local config to trigger the executable launch
    create_local_config(
        "test-agent".to_string(),
        "fake_variable: value".to_string(),
        dirs.local_dir(),
    );

    // Start Agent Control
    let agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

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
