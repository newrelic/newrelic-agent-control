use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::effective_config::check_latest_effective_config_is_expected;
use crate::common::health::check_latest_health_status_was_healthy;
use crate::common::remote_config_status::check_latest_remote_config_status_is_expected;
use crate::common::{retry::retry, runtime::tokio_runtime};
use crate::on_host::consts::NO_CONFIG;
use crate::on_host::tools::base_paths::TempBasePaths;
use crate::on_host::tools::config::{AgentControlConfigBuilder, create_file, create_local_config};
use crate::on_host::tools::config::{create_remote_config, load_remote_config_content};
use crate::on_host::tools::custom_agent_type::CustomAgentType;
use crate::on_host::tools::instance_id::get_instance_id;
use fake_opamp_server::FakeServer;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::defaults::{
    AGENT_CONTROL_ID, FOLDER_NAME_FLEET_DATA, STORE_KEY_OPAMP_DATA_CONFIG,
};
use newrelic_agent_control::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;
use newrelic_agent_control::agent_type::variable::namespace::Namespace;
use newrelic_agent_control::on_host::file_store::build_config_name;
use newrelic_agent_control::values::config::RemoteConfig;
use newrelic_agent_control::values::yaml_config::YAMLConfig;
use opamp_client::opamp::proto::RemoteConfigStatuses;
use std::env;
use std::time::Duration;

/// OpAMP is enabled but there is no remote configuration
/// - Local configuration (with no agents) is used
/// - Effective configuration for the agent-control is reported
/// - Healthy status is reported
#[test]
fn onhost_opamp_agent_control_local_effective_config() {
    // Given a agent-control without agents and opamp configured.
    let opamp_server = FakeServer::start(tokio_runtime().handle());

    let dirs = TempBasePaths::new();

    AgentControlConfigBuilder::basic(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .write(dirs.local_dir());

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    let agent_control_instance_id = get_instance_id(&AgentID::AgentControl, dirs.base_paths());

    retry(60, Duration::from_secs(1), || {
        let expected_config = "agents: {}\n";

        check_latest_effective_config_is_expected(
            &opamp_server,
            &agent_control_instance_id,
            expected_config.to_string(),
        )?;
        check_latest_health_status_was_healthy(&opamp_server, &agent_control_instance_id)
    });
}

/// Given a agent-control whose local configuration has no agents and then a valid remote configuration with an agent
/// is set through OpAMP:
/// - The corresponding files in the filesystem are created
/// - The corresponding effective config is reported for the agent control
/// - The agent control reports healthy
/// - The subagent reports healthy
#[test]
fn onhost_opamp_agent_control_remote_effective_config() {
    // Given a agent-control without agents and opamp configured.

    let mut opamp_server = FakeServer::start(tokio_runtime().handle());

    let dirs = TempBasePaths::new();

    AgentControlConfigBuilder::basic(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .write(dirs.local_dir());

    // Add custom agent_type to registry
    let sleep_agent_type = CustomAgentType::default().build(dirs.local_dir());

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    let agent_control_instance_id = get_instance_id(&AgentID::AgentControl, dirs.base_paths());

    let agents = format!(
        r#"
agents:
  nr-sleep-agent:
    agent_type: "{sleep_agent_type}"
"#
    );

    // When a new config with an agent is received from OpAMP
    opamp_server.set_config_response(agent_control_instance_id.clone(), agents.as_str());

    // Then the config should be updated in the remote filesystem.
    let expected_config = format!(
        r#"agents:
  nr-sleep-agent:
    agent_type: "{sleep_agent_type}"
"#
    );
    let expected_config_parsed =
        serde_saphyr::from_str::<YAMLConfig>(expected_config.as_str()).unwrap();

    retry(60, Duration::from_secs(1), || {
        let remote_file = dirs
            .remote_dir()
            .join(FOLDER_NAME_FLEET_DATA)
            .join(AGENT_CONTROL_ID)
            .join(build_config_name(STORE_KEY_OPAMP_DATA_CONFIG));
        let remote_config = std::fs::read_to_string(remote_file.as_path())
            .unwrap_or("config: \nhash: a-hash\nstate: applying\n".to_string());
        let content_parsed =
            serde_saphyr::from_str::<RemoteConfig>(remote_config.as_str()).unwrap();
        if content_parsed.config != expected_config_parsed {
            return Err(format!(
                "Agent Control config not as expected, Expected: {expected_config:?}, Found: {remote_config:?}",
            )
            .into());
        }

        check_latest_effective_config_is_expected(
            &opamp_server,
            &agent_control_instance_id,
            serde_saphyr::to_string(&content_parsed.config).unwrap(),
        )?;
        check_latest_health_status_was_healthy(&opamp_server, &agent_control_instance_id)
    });

    let subagent_instance_id = get_instance_id(
        &AgentID::try_from("nr-sleep-agent").unwrap(),
        dirs.base_paths(),
    );

    // The sub-agent waits for the remote config to be set, it cannot be empty since it would default to local
    // which does not exist.
    opamp_server.set_config_response(subagent_instance_id.clone(), "fake_variable: value");
    retry(60, Duration::from_secs(1), || {
        check_latest_health_status_was_healthy(&opamp_server, &subagent_instance_id)
    });
}

/// Given a agent-control whose local configuration has no agents and then a valid remote configuration with no agents
/// and an unknown field is set. The unknown should be ignored and the corresponding effective configuration reported.
#[test]
fn onhost_opamp_agent_control_accepts_unknown_fields_on_remote_config() {
    // Given a agent-control without agents and opamp configured.

    let mut opamp_server = FakeServer::start(tokio_runtime().handle());

    let dirs = TempBasePaths::new();

    AgentControlConfigBuilder::basic(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .write(dirs.local_dir());

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    let agent_control_instance_id = get_instance_id(&AgentID::AgentControl, dirs.base_paths());

    // When a new config with an agent is received from OpAMP
    opamp_server.set_config_response(
        agent_control_instance_id.clone(),
        r#"
agents: {}
non-existing: {}
"#,
    );

    retry(60, Duration::from_secs(1), || {
        {
            // And effective_config should return the initial local one
            let expected_config = "agents: {}\n";

            // TODO: Study if we want to fail if extra fields are present, now, as long as there is
            // a correct field `agents` is unmarshalled ignoring the rest and sets status as applied.
            check_latest_remote_config_status_is_expected(
                &opamp_server,
                &agent_control_instance_id,
                RemoteConfigStatuses::Applied as i32,
            )?;

            check_latest_effective_config_is_expected(
                &opamp_server,
                &agent_control_instance_id,
                expected_config.to_string(),
            )
        }
    });
}

/// The agent control is configured with one agent whose local configuration contains an environment variable
/// placeholder. This test checks that the effective config is reported as expected (and it does not included
/// the environment variable expanded).
#[test]
fn onhost_opamp_sub_agent_local_effective_config_with_env_var() {
    // Given a agent-control with a custom-agent running a sleep command with opamp configured.

    let opamp_server = FakeServer::start(tokio_runtime().handle());

    let dirs = TempBasePaths::new();

    let sleep_agent_type = CustomAgentType::default().build(dirs.local_dir());

    let agents = format!(
        r#"
  nr-sleep-agent:
    agent_type: "{sleep_agent_type}"
"#
    );

    AgentControlConfigBuilder::basic(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_agents(agents.to_string())
        .write(dirs.local_dir());

    // And the custom-agent has local config values
    let agent_id = "nr-sleep-agent";

    // Having an env_var placeholder and the corresponding env_var set in order to
    // check that is not expanded on the effective config
    unsafe { env::set_var("my_env_var", "my-value") };

    let values_config = format!(
        "fake_variable: ${{{}}}",
        Namespace::EnvironmentVariable.namespaced_name("my_env_var")
    );
    create_local_config(
        agent_id.to_string(),
        values_config.to_string(),
        dirs.local_dir(),
    );

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);
    let sub_agent_instance_id =
        get_instance_id(&AgentID::try_from(agent_id).unwrap(), dirs.base_paths());

    retry(60, Duration::from_secs(1), || {
        {
            // Then the retrieved effective config should match the expected local cfg
            let expected_config = format!(
                "fake_variable: ${{{}}}\n",
                Namespace::EnvironmentVariable.namespaced_name("my_env_var")
            );

            check_latest_effective_config_is_expected(
                &opamp_server,
                &sub_agent_instance_id,
                expected_config,
            )
        }
    });

    unsafe { env::remove_var("my_env_var") };
}

/// The agent-control is configured with on agent with local configuration and a remote configuration was also set for the
/// corresponding sub-agent. This test checks that the latest effective config reported corresponds to the remote.
#[test]
fn onhost_opamp_sub_agent_remote_effective_config() {
    // Given a agent-control with a custom-agent running a sleep command with opamp configured.

    let opamp_server = FakeServer::start(tokio_runtime().handle());

    let dirs = TempBasePaths::new();

    let sleep_agent_type = CustomAgentType::default().build(dirs.local_dir());

    let agents = format!(
        r#"
  nr-sleep-agent:
    agent_type: "{sleep_agent_type}"
"#
    );

    AgentControlConfigBuilder::basic(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_agents(agents.to_string())
        .write(dirs.local_dir());

    // And the custom-agent has local config values
    let agent_id = "nr-sleep-agent";
    let local_values_config = "fake_variable: from local\n";
    create_local_config(
        agent_id.to_string(),
        local_values_config.to_string(),
        dirs.local_dir(),
    );

    // And the custom-agent has also remote config values
    let remote_values_config_body = "fake_variable: from remote\n";
    let remote_values_config =
        format!("config:\n  {remote_values_config_body}hash: hash-test\nstate: applying\n");
    create_remote_config(
        agent_id.to_string(),
        remote_values_config.to_string(),
        dirs.remote_dir(),
    );

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    let sub_agent_instance_id =
        get_instance_id(&AgentID::try_from(agent_id).unwrap(), dirs.base_paths());

    retry(60, Duration::from_secs(1), || {
        {
            // Then the retrieved effective config should match the expected remote cfg
            check_latest_effective_config_is_expected(
                &opamp_server,
                &sub_agent_instance_id,
                remote_values_config_body.to_string(),
            )
        }
    });
}

/// There is a agent control with a sub agent configured whose configuration is empty (it exists but id doesn't contain
/// any value, if it didn't exist the supervisor would not start), we expect the empty configuration
/// to be reported as effective configuration for the sub-agent.
#[test]
fn onhost_opamp_sub_agent_empty_local_effective_config() {
    // Given a agent-control with a custom-agent running a sleep command with opamp configured.

    let opamp_server = FakeServer::start(tokio_runtime().handle());

    let dirs = TempBasePaths::new();

    let sleep_agent_type = CustomAgentType::default().build(dirs.local_dir());

    let agents = format!(
        r#"
  nr-sleep-agent:
    agent_type: "{sleep_agent_type}"
"#
    );

    AgentControlConfigBuilder::basic(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_agents(agents.to_string())
        .write(dirs.local_dir());

    // And the custom-agent has empty config values
    let agent_id = "nr-sleep-agent";
    create_local_config(
        agent_id.to_string(),
        NO_CONFIG.to_string(), // local empty config
        dirs.local_dir(),
    );

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    let sub_agent_instance_id =
        get_instance_id(&AgentID::try_from(agent_id).unwrap(), dirs.base_paths());

    retry(60, Duration::from_secs(1), || {
        {
            // Then the retrieved effective config should be empty
            let expected_config = "";

            check_latest_effective_config_is_expected(
                &opamp_server,
                &sub_agent_instance_id,
                expected_config.to_string(),
            )
        }
    });
}

/// There is a Sub Agent without executables and with valid remote config
/// - Local configuration (with no agents) is used
/// - Effective configuration for the agent-control is reported
/// - Healthy status is reported
///
/// A remote config is sent:
/// - Effective configuration updates to remote config
/// - Stored retrieves latest applied remote config
/// - Healthy status is reported
#[test]
fn onhost_executable_less_reports_local_effective_config() {
    // Given a agent-control without agents and opamp configured.

    let mut opamp_server = FakeServer::start(tokio_runtime().handle());

    let dirs = TempBasePaths::new();

    let health_file_path = dirs.local_dir().join("health_file.yaml");
    // Add custom agent_type to registry
    let health_agent_type_config = format!(
        r#"
interval: 2s
initial_delay: 0s
timeout: 1s
file:
  path: '{}'
  should_be_present: true
  unhealthy_string: ".*(unhealthy|fatal|error).*"
"#,
        health_file_path.to_string_lossy()
    );
    let agent_type_wo_exec = CustomAgentType::default()
        .with_executables(None)
        .with_version(None)
        .with_health(Some(&health_agent_type_config))
        .build(dirs.local_dir());

    let agents = format!(
        r#"
agents:
  no-executables:
    agent_type: "{agent_type_wo_exec}"
"#
    );

    AgentControlConfigBuilder::basic(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_agents(agents.to_string())
        .write(dirs.local_dir());

    let sub_agent_id = AgentID::try_from("no-executables").unwrap();
    let local_values_config = "fake_variable: valid local config\n";
    create_local_config(
        sub_agent_id.to_string(),
        local_values_config.to_string(),
        dirs.local_dir(),
    );

    // create sub agent health file as healthy
    create_file(
        r#"
healthy: true
status: "healthy-message"
start_time_unix_nano: 1725444000
status_time_unix_nano: 1725444001
    "#
        .to_string(),
        health_file_path.clone(),
    );

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    let sub_agent_instance_id = get_instance_id(&sub_agent_id, dirs.base_paths());

    retry(20, Duration::from_secs(1), || {
        check_latest_effective_config_is_expected(
            &opamp_server,
            &sub_agent_instance_id,
            local_values_config.to_string(),
        )?;
        check_latest_health_status_was_healthy(&opamp_server, &sub_agent_instance_id)
    });

    // Send remote configuration
    let remote_config = "fake_variable: valid remote config\n";
    // let remote_config = "fake_variable: valid remote config\n";
    opamp_server.set_config_response(sub_agent_instance_id.clone(), remote_config);

    retry(30, Duration::from_secs(1), || {
        check_latest_effective_config_is_expected(
            &opamp_server,
            &sub_agent_instance_id,
            remote_config.to_string(),
        )?;

        let Some(actual_remote_config) =
            load_remote_config_content(&sub_agent_id, dirs.base_paths())
        else {
            return Err("not the expected content for first config".into());
        };

        if actual_remote_config != remote_config {
            return Err("not the expected content for first config".into());
        }
        check_latest_health_status_was_healthy(&opamp_server, &sub_agent_instance_id)
    });
}
