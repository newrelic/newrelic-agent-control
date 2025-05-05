#![cfg(unix)]
use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::effective_config::check_latest_effective_config_is_expected;
use crate::common::health::check_latest_health_status_was_healthy;
use crate::common::opamp::ConfigResponse;
use crate::common::remote_config_status::check_latest_remote_config_status_is_expected;
use crate::common::{opamp::FakeServer, retry::retry};
use crate::on_host::tools::config::load_remote_config_content;
use crate::on_host::tools::config::{
    create_agent_control_config, create_file, create_sub_agent_values,
};
use crate::on_host::tools::custom_agent_type::{
    get_agent_type_custom, get_agent_type_without_executables,
};
use crate::on_host::tools::instance_id::get_instance_id;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::config::AgentControlDynamicConfig;
use newrelic_agent_control::agent_control::defaults::AGENT_CONTROL_CONFIG_FILENAME;
use newrelic_agent_control::agent_control::run::{BasePaths, Environment};
use newrelic_agent_control::agent_type::variable::namespace::Namespace;
use opamp_client::opamp::proto::RemoteConfigStatuses;
use std::env;
use std::time::Duration;
use tempfile::tempdir;

/// OpAMP is enabled but there is no remote configuration
/// - Local configuration (with no agents) is used
/// - Effective configuration for the agent-control is reported
/// - Healthy status is reported
#[test]
fn onhost_opamp_agent_control_local_effective_config() {
    // Given a agent-control without agents and opamp configured.

    let opamp_server = FakeServer::start_new();

    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    let agents = "{}";
    create_agent_control_config(
        opamp_server.endpoint(),
        agents.to_string(),
        local_dir.path().to_path_buf(),
        opamp_server.cert_file_path(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };
    let base_paths = base_paths.clone();

    let _agent_control =
        start_agent_control_with_custom_config(base_paths.clone(), Environment::OnHost);

    let agent_control_instance_id = get_instance_id(&AgentID::new_agent_control_id(), base_paths);

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

    let mut opamp_server = FakeServer::start_new();

    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    let agents = "{}";
    create_agent_control_config(
        opamp_server.endpoint(),
        agents.to_string(),
        local_dir.path().to_path_buf(),
        opamp_server.cert_file_path(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };

    // Add custom agent_type to registry
    let sleep_agent_type = get_agent_type_custom(
        local_dir.path().to_path_buf(),
        "sh",
        "tests/on_host/data/trap_term_sleep_60.sh",
    );

    let _agent_control =
        start_agent_control_with_custom_config(base_paths.clone(), Environment::OnHost);

    let agent_control_instance_id =
        get_instance_id(&AgentID::new_agent_control_id(), base_paths.clone());

    let agents = format!(
        r#"
agents:
  nr-sleep-agent:
    agent_type: "{}"
"#,
        sleep_agent_type
    );

    // When a new config with an agent is received from OpAMP
    opamp_server.set_config_response(
        agent_control_instance_id.clone(),
        ConfigResponse::from(agents.as_str()),
    );

    // Then the config should be updated in the remote filesystem.
    let expected_config = format!(
        r#"agents:
  nr-sleep-agent:
    agent_type: "{}"
"#,
        sleep_agent_type
    );

    let expected_config_parsed =
        serde_yaml::from_str::<AgentControlDynamicConfig>(expected_config.as_str()).unwrap();

    retry(60, Duration::from_secs(1), || {
        let remote_file = remote_dir.path().join(AGENT_CONTROL_CONFIG_FILENAME);
        let remote_config =
            std::fs::read_to_string(remote_file.as_path()).unwrap_or("agents:".to_string());
        let content_parsed =
            serde_yaml::from_str::<AgentControlDynamicConfig>(remote_config.as_str()).unwrap();
        if content_parsed != expected_config_parsed {
            return Err(format!(
                "Agent Control config not as expected, Expected: {:?}, Found: {:?}",
                expected_config, remote_config,
            )
            .into());
        }

        check_latest_effective_config_is_expected(
            &opamp_server,
            &agent_control_instance_id,
            remote_config,
        )?;
        check_latest_health_status_was_healthy(&opamp_server, &agent_control_instance_id)
    });

    let subagent_instance_id =
        get_instance_id(&AgentID::new("nr-sleep-agent").unwrap(), base_paths.clone());

    // The sub-agent waits for the remote config to be set, it cannot be empty since it would default to local
    // which does not exist.
    opamp_server.set_config_response(
        subagent_instance_id.clone(),
        ConfigResponse::from("fake_variable: value"),
    );
    retry(60, Duration::from_secs(1), || {
        check_latest_health_status_was_healthy(&opamp_server, &subagent_instance_id)
    });
}

/// Given a agent-control whose local configuration has no agents and then a valid remote configuration with no agents
/// and an unknown field is set. The unknown should be ignored and the corresponding effective configuration reported.
#[test]
fn onhost_opamp_agent_control_remote_config_with_unknown_field() {
    // Given a agent-control without agents and opamp configured.

    let mut opamp_server = FakeServer::start_new();

    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    let agents = "{}";
    create_agent_control_config(
        opamp_server.endpoint(),
        agents.to_string(),
        local_dir.path().to_path_buf(),
        opamp_server.cert_file_path(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };

    let _agent_control =
        start_agent_control_with_custom_config(base_paths.clone(), Environment::OnHost);

    let agent_control_instance_id = get_instance_id(&AgentID::new_agent_control_id(), base_paths);

    // When a new config with an agent is received from OpAMP
    opamp_server.set_config_response(
        agent_control_instance_id.clone(),
        ConfigResponse::from(
            r#"
agents: {}
non-existing: {}
"#,
        ),
    );

    retry(60, Duration::from_secs(1), || {
        {
            // Then the config should be updated in the remote filesystem.
            let expected_containing = "non-existing: {}";

            let remote_file = remote_dir.path().join(AGENT_CONTROL_CONFIG_FILENAME);
            let remote_config =
                std::fs::read_to_string(remote_file.as_path()).unwrap_or("agents:".to_string());
            if !remote_config.contains(expected_containing) {
                return Err(format!(
                    "Agent Control config not as expected, Expected containing: {:?}, Config Found: {:?}",
                    expected_containing, remote_config,
                )
                .into());
            }

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

    let opamp_server = FakeServer::start_new();

    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    let sleep_agent_type = get_agent_type_custom(
        local_dir.path().to_path_buf(),
        "sh",
        "tests/on_host/data/trap_term_sleep_60.sh",
    );

    let agents = format!(
        r#"
  nr-sleep-agent:
    agent_type: "{}"
"#,
        sleep_agent_type
    );

    create_agent_control_config(
        opamp_server.endpoint(),
        agents.to_string(),
        local_dir.path().to_path_buf(),
        opamp_server.cert_file_path(),
    );

    // And the custom-agent has local config values
    let agent_id = "nr-sleep-agent";

    // Having an env_var placeholder and the corresponding env_var set in order to
    // check that is not expanded on the effective config
    unsafe { env::set_var("my_env_var", "my-value") };

    let values_config = format!(
        "fake_variable: ${{{}}}",
        Namespace::EnvironmentVariable.namespaced_name("my_env_var")
    );
    create_sub_agent_values(
        agent_id.to_string(),
        values_config.to_string(),
        local_dir.path().to_path_buf(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };
    let _agent_control =
        start_agent_control_with_custom_config(base_paths.clone(), Environment::OnHost);
    let sub_agent_instance_id = get_instance_id(&AgentID::new(agent_id).unwrap(), base_paths);

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

    let opamp_server = FakeServer::start_new();

    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    let sleep_agent_type = get_agent_type_custom(
        local_dir.path().to_path_buf(),
        "sh",
        "tests/on_host/data/trap_term_sleep_60.sh",
    );

    let agents = format!(
        r#"
  nr-sleep-agent:
    agent_type: "{}"
"#,
        sleep_agent_type
    );

    create_agent_control_config(
        opamp_server.endpoint(),
        agents.to_string(),
        local_dir.path().to_path_buf(),
        opamp_server.cert_file_path(),
    );

    // And the custom-agent has local config values
    let agent_id = "nr-sleep-agent";
    let local_values_config = "fake_variable: from local\n";
    create_sub_agent_values(
        agent_id.to_string(),
        local_values_config.to_string(),
        local_dir.path().to_path_buf(),
    );

    // And the custom-agent has also remote config values
    let remote_values_config = "fake_variable: from remote\n";
    create_sub_agent_values(
        agent_id.to_string(),
        remote_values_config.to_string(),
        remote_dir.path().to_path_buf(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };
    let _agent_control =
        start_agent_control_with_custom_config(base_paths.clone(), Environment::OnHost);

    let sub_agent_instance_id = get_instance_id(&AgentID::new(agent_id).unwrap(), base_paths);

    retry(60, Duration::from_secs(1), || {
        {
            // Then the retrieved effective config should match the expected remote cfg
            check_latest_effective_config_is_expected(
                &opamp_server,
                &sub_agent_instance_id,
                remote_values_config.to_string(),
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

    let opamp_server = FakeServer::start_new();

    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    let sleep_agent_type = get_agent_type_custom(
        local_dir.path().to_path_buf(),
        "sh",
        "tests/on_host/data/trap_term_sleep_60.sh",
    );

    let agents = format!(
        r#"
  nr-sleep-agent:
    agent_type: "{}"
"#,
        sleep_agent_type
    );

    create_agent_control_config(
        opamp_server.endpoint(),
        agents.to_string(),
        local_dir.path().to_path_buf(),
        opamp_server.cert_file_path(),
    );

    // And the custom-agent has empty config values
    let agent_id = "nr-sleep-agent";
    create_sub_agent_values(
        agent_id.to_string(),
        "".to_string(), // local empty config
        local_dir.path().into(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };
    let _agent_control =
        start_agent_control_with_custom_config(base_paths.clone(), Environment::OnHost);

    let sub_agent_instance_id = get_instance_id(&AgentID::new(agent_id).unwrap(), base_paths);

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

    let mut opamp_server = FakeServer::start_new();

    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    let health_file_path = local_dir.path().join("health_file.yaml");
    // Add custom agent_type to registry
    let agent_type_wo_exec = get_agent_type_without_executables(
        local_dir.path().to_path_buf(),
        health_file_path.as_path(),
    );

    let agents = format!(
        r#"
agents:
  no-executables:
    agent_type: "{}"
"#,
        agent_type_wo_exec
    );

    create_agent_control_config(
        opamp_server.endpoint(),
        agents.to_string(),
        local_dir.path().to_path_buf(),
        opamp_server.cert_file_path(),
    );

    let sub_agent_id = AgentID::new("no-executables").unwrap();
    let local_values_config = "fake_variable: valid local config\n";
    create_sub_agent_values(
        sub_agent_id.to_string(),
        local_values_config.to_string(),
        local_dir.path().to_path_buf(),
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

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };
    let _agent_control =
        start_agent_control_with_custom_config(base_paths.clone(), Environment::OnHost);

    let sub_agent_instance_id = get_instance_id(&sub_agent_id, base_paths.clone());

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
    opamp_server.set_config_response(
        sub_agent_instance_id.clone(),
        ConfigResponse::from(remote_config),
    );

    retry(30, Duration::from_secs(1), || {
        check_latest_effective_config_is_expected(
            &opamp_server,
            &sub_agent_instance_id,
            remote_config.to_string(),
        )?;

        let Some(actual_remote_config) =
            load_remote_config_content(&sub_agent_id, base_paths.clone())
        else {
            return Err("not the expected content for first config".into());
        };

        if actual_remote_config != remote_config {
            return Err("not the expected content for first config".into());
        }
        check_latest_health_status_was_healthy(&opamp_server, &sub_agent_instance_id)
    });
}
