//! Scenarios driving the experimental `--experimental-detach-sub-agents-on-stop` behavior.
//!
//! The default behavior (sub-agent processes are terminated when Agent Control stops) is covered by
//! `no_orphan_processes`.

use crate::common::agent_control::start_agent_control_detaching_sub_agents_on_stop;
use crate::common::base_paths::TempBasePaths;
use crate::common::process_finder::{KillProcessesOnDrop, find_processes_by_pattern};
use crate::common::remote_config_status::check_latest_remote_config_status_is_expected;
use crate::common::retry::{retry, retry_never};
use crate::common::runtime::tokio_runtime;
use crate::on_host::tools::config::{OnHostAgentControlConfigBuilder, create_local_config};
use crate::on_host::tools::custom_agent_type::OnHostCustomAgentTypeBuilder;
use crate::on_host::tools::instance_id::get_instance_id;
use fake_opamp_server::FakeServer;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;
use opamp_client::opamp::proto::RemoteConfigStatuses;
use std::time::Duration;

const AGENT_ID: &str = "test-agent";

/// Writes the config of an Agent Control running one sub-agent whose executable sleeps for
/// `seconds`, and returns the pattern that finds that executable among the running processes.
/// Each test uses its own `seconds` so tests running in parallel don't see each other's processes.
fn configure_sleeping_sub_agent(
    dirs: &TempBasePaths,
    opamp_server: &FakeServer,
    seconds: u32,
) -> String {
    let executables = cfg_select! {
        target_family = "unix" => format!(r#"[{{"id": "long-sleep", "path": "sleep", "args": ["{seconds}"]}}]"#),
        target_family = "windows" => format!(
            r#"[{{"id": "long-sleep", "path": "powershell", "args": ["-Command", "Start-Sleep", "-Seconds", "{seconds}"]}}]"#
        )
    };

    let agent_type = OnHostCustomAgentTypeBuilder::empty()
        .with_executables(Some(&executables))
        .write(dirs.local_dir());

    OnHostAgentControlConfigBuilder::new(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_agent(AGENT_ID, agent_type)
        .write(dirs.local_dir());

    // Local config that triggers the executable launch.
    create_local_config(
        AGENT_ID.to_string(),
        "fake_variable: value".to_string(),
        dirs.local_dir(),
    );

    cfg_select! {
        target_family = "unix" => format!("sleep {seconds}"),
        target_family = "windows" => format!("Start-Sleep -Seconds {seconds}")
    }
}

/// Waits until exactly one process matches the pattern and returns its PID.
fn wait_for_single_process(pattern: &str) -> String {
    retry(60, Duration::from_secs(1), || {
        let pids = find_processes_by_pattern(pattern);
        match pids.as_slice() {
            [pid] => Ok(pid.clone()),
            _ => Err(format!("expected exactly one '{pattern}' process, found: {pids:?}").into()),
        }
    })
}

/// Given Agent Control runs a sub-agent with the experimental flag enabled, when Agent Control
/// stops, then the sub-agent process keeps running, and it is the same process.
#[test]
fn test_sub_agent_survives_agent_control_stop_when_detaching() {
    let opamp_server = FakeServer::start(tokio_runtime().handle());
    let dirs = TempBasePaths::default();
    let pattern = configure_sleeping_sub_agent(&dirs, &opamp_server, 3601);
    // Kills the process this test deliberately leaves behind, even if an assertion fails.
    let _cleanup = KillProcessesOnDrop(pattern.clone());

    let agent_control = start_agent_control_detaching_sub_agents_on_stop(
        dirs.base_paths(),
        AGENT_CONTROL_MODE_ON_HOST,
    );
    let pid_before = wait_for_single_process(&pattern);

    // Stop Agent Control and wait for it to exit.
    drop(agent_control);

    // The process must not be terminated late either, so check it for a while.
    retry_never(5, Duration::from_secs(1), || {
        let pids = find_processes_by_pattern(&pattern);
        if pids != [pid_before.clone()] {
            return Err(format!(
                "expected only process {pid_before} to be running, found: {pids:?}"
            )
            .into());
        }
        Ok(())
    });
}

/// Given Agent Control runs a sub-agent with the experimental flag enabled, when the sub-agent is
/// removed by a remote config, then its process is terminated: the flag only applies when Agent
/// Control exits.
#[test]
fn test_removed_sub_agent_is_terminated_when_detaching() {
    let mut opamp_server = FakeServer::start(tokio_runtime().handle());
    let dirs = TempBasePaths::default();
    let pattern = configure_sleeping_sub_agent(&dirs, &opamp_server, 3602);
    let _cleanup = KillProcessesOnDrop(pattern.clone());

    let _agent_control = start_agent_control_detaching_sub_agents_on_stop(
        dirs.base_paths(),
        AGENT_CONTROL_MODE_ON_HOST,
    );
    wait_for_single_process(&pattern);

    // Remove the sub-agent.
    let ac_instance_id = get_instance_id(&AgentID::AgentControl, dirs.base_paths());
    opamp_server.set_config_response(ac_instance_id.clone(), "agents: {}");
    retry(60, Duration::from_secs(1), || {
        check_latest_remote_config_status_is_expected(
            &opamp_server,
            &ac_instance_id,
            RemoteConfigStatuses::Applied as i32,
        )
    });

    retry(30, Duration::from_secs(1), || {
        let pids = find_processes_by_pattern(&pattern);
        if pids.is_empty() {
            Ok(())
        } else {
            Err(format!("the removed sub-agent is still running: {pids:?}").into())
        }
    });
}
