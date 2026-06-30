use std::time::Duration;

use crate::{
    common::{
        agent_control::start_agent_control_with_custom_config, base_paths::TempBasePaths,
        health::check_latest_health_status, retry::retry, runtime::tokio_runtime,
    },
    on_host::tools::{
        config::OnHostAgentControlConfigBuilder, custom_agent_type::CustomAgentType,
        instance_id::get_instance_id,
    },
};
use fake_opamp_server::FakeServer;
use newrelic_agent_control::agent_control::{
    agent_id::AgentID, run::on_host::AGENT_CONTROL_MODE_ON_HOST,
};

#[test]
fn onhost_subagent_multiple_executables_some_failed_launching() {
    let mut opamp_server = FakeServer::start(tokio_runtime().handle());

    let dirs = TempBasePaths::default();

    // Add custom agent_type to registry
    let sleep_agent_type = CustomAgentType::default()
        .with_executables(Some(
            r#"[
                {"id": "trap-term-sleep", "path": "sh", "args": ["tests/on_host/data/sleep_60.sh"]},
                {"id": "unknown", "path": "unknown-command"}
            ]"#,
        ))
        .with_health(Some(r#"{"interval": "1s", "initial_delay": "2s"}"#))
        .build(dirs.local_dir());

    let agents = format!(
        r#"
agents:
  nr-sleep-agent:
    agent_type: "{sleep_agent_type}"
"#
    );

    OnHostAgentControlConfigBuilder::new(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_agents(agents)
        .write(dirs.local_dir());

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    let subagent_instance_id = get_instance_id(
        &AgentID::try_from("nr-sleep-agent").unwrap(),
        dirs.base_paths(),
    );
    // Set sub-agent configuration remotely
    opamp_server.set_config_response(subagent_instance_id.clone(), "fake_variable: value");

    retry(60, Duration::from_secs(1), || {
        check_latest_health_status(&opamp_server, &subagent_instance_id, |h| {
            !h.healthy && h.last_error.contains("unknown")
        })
    });
}

#[test]
fn onhost_subagent_multiple_executables_some_commands_failed_after_max_retries() {
    let mut opamp_server = FakeServer::start(tokio_runtime().handle());

    let dirs = TempBasePaths::default();

    // Add custom agent_type to registry
    let sleep_agent_type = CustomAgentType::default()
        .with_executables(Some(
            r#"[
                {"id": "trap-term-sleep", "path": "sh", "args": ["tests/on_host/data/sleep_60.sh"]},
                {"id": "failing-process", "path": "sh", args: ["tests/on_host/data/sleep_and_fail.sh"],
                 "restart_policy": {"backoff_strategy": {"type": "fixed", "backoff_delay": "1s", "max_retries": 2}}
                }
            ]"#,
        ))
        .with_health(Some(r#"{"interval": "1s", "initial_delay": "2s"}"#))
        .build(dirs.local_dir());

    let agents = format!(
        r#"
agents:
  nr-sleep-agent:
    agent_type: "{sleep_agent_type}"
"#
    );

    OnHostAgentControlConfigBuilder::new(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_agents(agents)
        .write(dirs.local_dir());

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    let subagent_instance_id = get_instance_id(
        &AgentID::try_from("nr-sleep-agent").unwrap(),
        dirs.base_paths(),
    );
    // Set sub-agent configuration remotely
    opamp_server.set_config_response(subagent_instance_id.clone(), "fake_variable: value");

    retry(60, Duration::from_secs(1), || {
        check_latest_health_status(&opamp_server, &subagent_instance_id, |h| {
            !h.healthy
                && h.last_error.contains("failing-process")
                && h.last_error.contains("Restart policy exceeded")
        })
    });
}
