use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::base_paths::TempBasePaths;
use crate::common::effective_config::check_latest_effective_config_is_expected;
use crate::common::retry::{retry, retry_never};
use crate::common::runtime::tokio_runtime;
use crate::on_host::consts::NO_CONFIG;
use crate::on_host::tools::config::OnHostAgentControlConfigBuilder;
use crate::on_host::tools::config::create_local_config;
use crate::on_host::tools::custom_agent_type::CustomAgentType;
use crate::on_host::tools::instance_id::get_instance_id;
use fake_opamp_server::FakeServer;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;
use std::time::Duration;

/// When an on-host sub-agent uses an agent type that does NOT define a `health:` block, no
/// `ComponentHealth` should be recorded for the sub-agent.
///
/// We check for the sub-agent's effective configuration to be reported, so that the no-health
/// assertion cannot pass for the wrong reason (i.e. the sub-agent never connecting to OpAMP).
#[test]
fn test_on_host_no_health_in_agent_type_reports_no_health_via_opamp() {
    let opamp_server = FakeServer::start(tokio_runtime().handle());

    let dirs = TempBasePaths::default();
    let sub_agent_id = AgentID::try_from("test-agent-no-health").unwrap();

    let agent_type = CustomAgentType::default()
        .with_health(None)
        .build(dirs.local_dir());

    let agents = format!(
        r#"
  {sub_agent_id}:
    agent_type: "{agent_type}"
"#
    );

    create_local_config(
        sub_agent_id.to_string(),
        NO_CONFIG.to_string(),
        dirs.local_dir(),
    );
    OnHostAgentControlConfigBuilder::new(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_agents(agents.to_string())
        .write(dirs.local_dir());

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    let sub_agent_instance_id = get_instance_id(&sub_agent_id, dirs.base_paths());

    // Cross-check: the sub-agent must actually be talking to OpAMP. Wait until
    // its (empty) effective configuration shows up on the server.
    retry(60, Duration::from_secs(1), || {
        check_latest_effective_config_is_expected(
            &opamp_server,
            &sub_agent_instance_id,
            "".to_string(),
        )
    });

    // No health status is expected
    retry_never(10, Duration::from_secs(1), || {
        match opamp_server.get_health_status(sub_agent_instance_id.clone()) {
            None => Ok(()),
            Some(health) => Err(format!(
                "Expected no ComponentHealth for sub-agent without `health:` in agent type, got: {health:?}"
            )
            .into()),
        }
    });
}
