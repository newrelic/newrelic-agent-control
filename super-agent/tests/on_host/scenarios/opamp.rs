use crate::common::effective_config::check_latest_effective_config_is_expected;
use crate::common::health::check_latest_health_status_was_healthy;
use crate::common::opamp::ConfigResponse;
use crate::common::remote_config_status::check_latest_remote_config_status_is_expected;
use crate::common::super_agent::start_super_agent_with_custom_config;
use crate::common::{opamp::FakeServer, retry::retry};
use crate::on_host::tools::config::{
    create_file, create_sub_agent_values, create_super_agent_config,
};
use crate::on_host::tools::custom_agent_type::{
    get_agent_type_custom, get_agent_type_without_executables,
};
use crate::on_host::tools::instance_id::get_instance_id;
use newrelic_super_agent::agent_type::variable::namespace::Namespace;
use newrelic_super_agent::super_agent::config::{AgentID, SuperAgentDynamicConfig};
use newrelic_super_agent::super_agent::defaults::{
    DYNAMIC_AGENT_TYPE_FILENAME, SUB_AGENT_DIR, SUPER_AGENT_CONFIG_FILE, VALUES_DIR, VALUES_FILE,
};
use newrelic_super_agent::super_agent::run::BasePaths;
use opamp_client::opamp::proto::RemoteConfigStatuses;
use std::env;
use std::path::Path;
use std::time::Duration;
use tempfile::tempdir;

/// OpAMP is enabled but there is no remote configuration
/// - Local configuration (with no agents) is used
/// - Effective configuration for the super-agent is reported
/// - Healthy status is reported
#[cfg(unix)]
#[test]
fn onhost_opamp_super_agent_local_effective_config() {
    // Given a super-agent without agents and opamp configured.
    let opamp_server = FakeServer::start_new();

    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    let agents = "{}";
    create_super_agent_config(
        opamp_server.endpoint(),
        agents.to_string(),
        local_dir.path().to_path_buf(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };
    let base_paths = base_paths.clone();

    let _super_agent = start_super_agent_with_custom_config(base_paths.clone());

    let super_agent_instance_id = get_instance_id(&AgentID::new_super_agent_id(), base_paths);

    retry(60, Duration::from_secs(1), || {
        let expected_config = "agents: {}\n";

        check_latest_effective_config_is_expected(
            &opamp_server,
            &super_agent_instance_id,
            expected_config.to_string(),
        )?;
        check_latest_health_status_was_healthy(&opamp_server, &super_agent_instance_id)
    });
}

/// Given a super-agent whose local configuration has no agents and then a valid remote configuration with an agent
/// is set through OpAMP:
/// - The corresponding files in the filesystem are created
/// - The corresponding effective config is reported for the super agent
/// - The super agent reports healthy
/// - The subagent reports healthy
#[cfg(unix)]
#[test]
fn onhost_opamp_super_agent_remote_effective_config() {
    // Given a super-agent without agents and opamp configured.

    let mut opamp_server = FakeServer::start_new();

    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    let agents = "{}";
    create_super_agent_config(
        opamp_server.endpoint(),
        agents.to_string(),
        local_dir.path().to_path_buf(),
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

    let _super_agent = start_super_agent_with_custom_config(base_paths.clone());

    let super_agent_instance_id =
        get_instance_id(&AgentID::new_super_agent_id(), base_paths.clone());

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
        super_agent_instance_id.clone(),
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
        serde_yaml::from_str::<SuperAgentDynamicConfig>(expected_config.as_str()).unwrap();

    let subagent_instance_id =
        get_instance_id(&AgentID::new("nr-sleep-agent").unwrap(), base_paths.clone());

    retry(60, Duration::from_secs(1), || {
        let remote_file = remote_dir.path().join(SUPER_AGENT_CONFIG_FILE);
        let remote_config =
            std::fs::read_to_string(remote_file.as_path()).unwrap_or("agents:".to_string());
        let content_parsed =
            serde_yaml::from_str::<SuperAgentDynamicConfig>(remote_config.as_str()).unwrap();
        if content_parsed != expected_config_parsed {
            return Err(format!(
                "Super agent config not as expected, Expected: {:?}, Found: {:?}",
                expected_config, remote_config,
            )
            .into());
        }

        check_latest_effective_config_is_expected(
            &opamp_server,
            &super_agent_instance_id,
            remote_config,
        )?;
        check_latest_health_status_was_healthy(&opamp_server, &super_agent_instance_id)?;
        check_latest_health_status_was_healthy(&opamp_server, &subagent_instance_id)
    });
}

/// Given a super-agent whose local configuration has no agents and then a valid remote configuration with no agents
/// and an unknown field is set. The unknown should be ignored and the corresponding effective configuration reported.
#[cfg(unix)]
#[test]
fn onhost_opamp_super_agent_remote_config_with_unknown_field() {
    // Given a super-agent without agents and opamp configured.
    let mut opamp_server = FakeServer::start_new();

    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    let agents = "{}";
    create_super_agent_config(
        opamp_server.endpoint(),
        agents.to_string(),
        local_dir.path().to_path_buf(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };

    let _super_agent = start_super_agent_with_custom_config(base_paths.clone());

    let super_agent_instance_id = get_instance_id(&AgentID::new_super_agent_id(), base_paths);

    // When a new config with an agent is received from OpAMP
    opamp_server.set_config_response(
        super_agent_instance_id.clone(),
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

            let remote_file = remote_dir.path().join(SUPER_AGENT_CONFIG_FILE);
            let remote_config =
                std::fs::read_to_string(remote_file.as_path()).unwrap_or("agents:".to_string());
            if !remote_config.contains(expected_containing) {
                return Err(format!(
                    "Super agent config not as expected, Expected containing: {:?}, Config Found: {:?}",
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
                &super_agent_instance_id,
                RemoteConfigStatuses::Applied as i32,
            )?;

            check_latest_effective_config_is_expected(
                &opamp_server,
                &super_agent_instance_id,
                expected_config.to_string(),
            )
        }
    });
}

/// The super agent is configured with one agent whose local configuration contains an environment variable
/// placeholder. This test checks that the effective config is reported as expected (and it does not included
/// the environment variable expanded).
#[cfg(unix)]
#[test]
fn onhost_opamp_sub_agent_local_effective_config_with_env_var() {
    // Given a super-agent with a custom-agent running a sleep command with opamp configured.
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

    create_super_agent_config(
        opamp_server.endpoint(),
        agents.to_string(),
        local_dir.path().to_path_buf(),
    );

    // And the custom-agent has local config values
    let agent_id = "nr-sleep-agent";

    // Having an env_var placeholder and the corresponding env_var set in order to
    // check that is not expanded on the effective config
    env::set_var("my_env_var", "my-value");

    let values_config = format!(
        "backoff_delay: ${{{}}}",
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
    let _super_agent = start_super_agent_with_custom_config(base_paths.clone());
    let sub_agent_instance_id = get_instance_id(&AgentID::new(agent_id).unwrap(), base_paths);

    retry(60, Duration::from_secs(1), || {
        {
            // Then the retrieved effective config should match the expected local cfg
            let expected_config = format!(
                "backoff_delay: ${{{}}}\n",
                Namespace::EnvironmentVariable.namespaced_name("my_env_var")
            );

            check_latest_effective_config_is_expected(
                &opamp_server,
                &sub_agent_instance_id,
                expected_config,
            )
        }
    });

    env::remove_var("my_env_var");
}

/// The super-agent is configured with on agent with local configuration and a remote configuration was also set for the
/// corresponding sub-agent. This test checks that the latest effective config reported corresponds to the remote.
#[cfg(unix)]
#[test]
fn onhost_opamp_sub_agent_remote_effective_config() {
    // Given a super-agent with a custom-agent running a sleep command with opamp configured.
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

    create_super_agent_config(
        opamp_server.endpoint(),
        agents.to_string(),
        local_dir.path().to_path_buf(),
    );

    // And the custom-agent has local config values
    let agent_id = "nr-sleep-agent";
    let local_values_config = "backoff_delay: 10s";
    create_sub_agent_values(
        agent_id.to_string(),
        local_values_config.to_string(),
        local_dir.path().to_path_buf(),
    );

    // And the custom-agent has also remote config values
    let remote_values_config = "backoff_delay: 40s";
    create_sub_agent_values(
        agent_id.to_string(),
        remote_values_config.to_string(),
        local_dir.path().to_path_buf(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };
    let _super_agent = start_super_agent_with_custom_config(base_paths.clone());

    let sub_agent_instance_id = get_instance_id(&AgentID::new(agent_id).unwrap(), base_paths);

    retry(60, Duration::from_secs(1), || {
        {
            // Then the retrieved effective config should match the expected remote cfg
            let expected_config = "backoff_delay: 40s\n";

            check_latest_effective_config_is_expected(
                &opamp_server,
                &sub_agent_instance_id,
                expected_config.to_string(),
            )
        }
    });
}

/// There is a super agent with a sub agent configured whose configuration is empty, we expect the empty configuration
/// to be reported as effective configuration for the sub-agent.
#[cfg(unix)]
#[test]
fn onhost_opamp_sub_agent_empty_local_effective_config() {
    // Given a super-agent with a custom-agent running a sleep command with opamp configured.
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

    create_super_agent_config(
        opamp_server.endpoint(),
        agents.to_string(),
        local_dir.path().to_path_buf(),
    );

    // And the custom-agent has no config values
    let agent_id = "nr-sleep-agent";

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };
    let _super_agent = start_super_agent_with_custom_config(base_paths.clone());

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

/// A super agent has a sub agent configured and the sub agent has a local configuration, then a **invalid** remote
/// configuration is set. This test checks:
/// - That the latest remote config status is failed.
/// - That latest effective configuration reported is the local one (which is valid).
#[cfg(unix)]
#[test]
fn onhost_opamp_sub_gent_wrong_remote_effective_config() {
    // Given a super-agent with a custom-agent running a sleep command with opamp configured.
    let mut opamp_server = FakeServer::start_new();

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

    create_super_agent_config(
        opamp_server.endpoint(),
        agents.to_string(),
        local_dir.path().to_path_buf(),
    );

    // And the custom-agent has local config values
    let agent_id = "nr-sleep-agent";
    let initial_values_config = "backoff_delay: 30s";
    create_sub_agent_values(
        agent_id.to_string(),
        initial_values_config.to_string(),
        local_dir.path().to_path_buf(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };
    let _super_agent = start_super_agent_with_custom_config(base_paths.clone());

    let sub_agent_instance_id = get_instance_id(&AgentID::new(agent_id).unwrap(), base_paths);

    // When a new incorrect config is received from OpAMP
    opamp_server.set_config_response(
        sub_agent_instance_id.clone(),
        ConfigResponse::from("config_agent: aa"),
    );

    retry(60, Duration::from_secs(1), || {
        {
            // Then the remote config should be created in the remote filesystem.
            let remote_file = remote_dir
                .path()
                .join(SUB_AGENT_DIR)
                .join(agent_id)
                .join(VALUES_DIR)
                .join(VALUES_FILE);
            if !remote_file.exists() {
                return Err("Remote config file should be created".into());
            }

            // And effective_config should return the initial local one
            let expected_config = format!("{}\n", initial_values_config);

            check_latest_remote_config_status_is_expected(
                &opamp_server,
                &sub_agent_instance_id,
                RemoteConfigStatuses::Failed as i32,
            )?;

            check_latest_effective_config_is_expected(
                &opamp_server,
                &sub_agent_instance_id,
                expected_config.to_string(),
            )
        }
    });
}

/// There is a Sub Agent without executables
/// OpAMP is enabled but there is no remote configuration
/// - Local configuration (with no agents) is used
/// - Effective configuration for the super-agent is reported
/// - Healthy status is reported
#[cfg(unix)]
#[test]
fn onhost_executable_less_reports_local_effective_config() {
    // Given a super-agent without agents and opamp configured.
    let opamp_server = FakeServer::start_new();

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

    create_super_agent_config(
        opamp_server.endpoint(),
        agents.to_string(),
        local_dir.path().to_path_buf(),
    );

    let sub_agent_id = AgentID::new("no-executables").unwrap();
    let local_values_config = "backoff_delay: 10s";
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
    let _super_agent = start_super_agent_with_custom_config(base_paths.clone());

    let sub_agent_instance_id = get_instance_id(&sub_agent_id, base_paths);

    retry(20, Duration::from_secs(1), || {
        let expected_config = "backoff_delay: 10s\n";

        check_latest_effective_config_is_expected(
            &opamp_server,
            &sub_agent_instance_id,
            expected_config.to_string(),
        )?;
        check_latest_health_status_was_healthy(&opamp_server, &sub_agent_instance_id)
    });
}

/// Given a super-agent with a sub-agent without supervised executables, it should be able
/// to persist the remote config messages from OpAMP. Furthermore, the corresponding
/// effective config should be properly reported.
#[cfg(unix)]
#[test]
fn test_config_without_supervisor() {
    let mut opamp_server = FakeServer::start_new();

    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");
    let sub_agent_id = AgentID::new("test-agent").unwrap();

    agent_type_without_executables(local_dir.path());

    let agents = r#"
  test-agent:
    agent_type: "test/test:0.0.0"
"#;

    create_super_agent_config(
        opamp_server.endpoint(),
        agents.to_string(),
        local_dir.path().to_path_buf(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };
    let _super_agent = start_super_agent_with_custom_config(base_paths.clone());

    let instance_id = get_instance_id(&sub_agent_id, base_paths.clone());

    // Send the first remote configuration
    let first_remote_config = "some_string: some value\n";
    opamp_server.set_config_response(
        instance_id.clone(),
        ConfigResponse::from(first_remote_config),
    );

    let sub_agent_instance_id =
        get_instance_id(&AgentID::new(&sub_agent_id).unwrap(), base_paths.clone());

    retry(30, Duration::from_secs(1), || {
        check_latest_effective_config_is_expected(
            &opamp_server,
            &sub_agent_instance_id,
            first_remote_config.to_string(),
        )?;
        let remote_config = crate::on_host::tools::config::get_remote_config_content(
            &sub_agent_id,
            base_paths.clone(),
        )?;
        if remote_config != first_remote_config {
            return Err("not the expected content for first config".into());
        }
        Ok(())
    });

    // Send another configuration
    let second_remote_config = "some_string: this is amazing\n";
    opamp_server.set_config_response(
        instance_id.clone(),
        ConfigResponse::from(second_remote_config),
    );

    retry(30, Duration::from_secs(1), || {
        check_latest_effective_config_is_expected(
            &opamp_server,
            &sub_agent_instance_id,
            second_remote_config.to_string(),
        )?;

        let remote_config = crate::on_host::tools::config::get_remote_config_content(
            &sub_agent_id,
            base_paths.clone(),
        )?;
        if remote_config != second_remote_config {
            return Err("not the expected content for second config".into());
        }
        Ok(())
    });
}

/// Given a super-agent with a sub-agent without supervised executables, it should be able
/// to receive an invalid config, and then a second valid one from OpAMP
#[cfg(unix)]
#[test]
fn test_invalid_config_without_supervisor() {
    let mut opamp_server = FakeServer::start_new();

    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");
    let sub_agent_id = AgentID::new("test-agent").unwrap();

    agent_type_without_executables(local_dir.path());

    let agents = r#"
  test-agent:
    agent_type: "test/test:0.0.0"
"#;

    create_super_agent_config(
        opamp_server.endpoint(),
        agents.to_string(),
        local_dir.path().to_path_buf(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };
    let _super_agent = start_super_agent_with_custom_config(base_paths.clone());

    let instance_id = get_instance_id(&sub_agent_id, base_paths.clone());

    // Send an invalid first remote configuration
    let first_remote_config = "this_does_not_exit: in the agent type\n";
    opamp_server.set_config_response(
        instance_id.clone(),
        ConfigResponse::from(first_remote_config),
    );

    let sub_agent_instance_id =
        get_instance_id(&AgentID::new(&sub_agent_id).unwrap(), base_paths.clone());

    retry(30, Duration::from_secs(1), || {
        check_latest_effective_config_is_expected(
            &opamp_server,
            &sub_agent_instance_id,
            "".to_string(), // The effective config should not be updated, since the configuration failed
        )?;
        let remote_config = crate::on_host::tools::config::get_remote_config_content(
            &sub_agent_id,
            base_paths.clone(),
        )?;
        if remote_config != first_remote_config {
            return Err("not the expected content".into());
        }
        Ok(())
    });

    // Send another configuration
    let second_remote_config = "some_string: this is amazing\n";
    opamp_server.set_config_response(
        instance_id.clone(),
        ConfigResponse::from(second_remote_config),
    );

    retry(30, Duration::from_secs(1), || {
        check_latest_effective_config_is_expected(
            &opamp_server,
            &sub_agent_instance_id,
            second_remote_config.to_string(), // Correct config leads to updated effective config
        )?;
        let remote_config = crate::on_host::tools::config::get_remote_config_content(
            &sub_agent_id,
            base_paths.clone(),
        )?;
        if remote_config != second_remote_config {
            return Err("not the expected content".into());
        }
        Ok(())
    });
}

////////////////////////////////
// Helpers
////////////////////////////////
pub(super) fn agent_type_without_executables(local_dir: &Path) {
    create_file(
        String::from(
            r#"
namespace: test
name: test
version: 0.0.0
variables:
  on_host:
    some_string:
      description: "some string"
      type: string
      required: true
deployment:
  on_host: {}
"#,
        ),
        local_dir.join(DYNAMIC_AGENT_TYPE_FILENAME),
    );
}
