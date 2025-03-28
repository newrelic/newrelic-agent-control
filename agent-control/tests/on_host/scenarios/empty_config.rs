use crate::{
    common::{
        agent_control::start_agent_control_with_custom_config,
        effective_config::check_latest_effective_config_is_expected, opamp::ConfigResponse,
        opamp::FakeServer, retry::retry,
    },
    on_host::tools::{
        config::{create_agent_control_config, create_sub_agent_values},
        custom_agent_type::get_agent_type_custom,
        instance_id::get_instance_id,
    },
};
use newrelic_agent_control::agent_control::{agent_id::AgentID, run::BasePaths};
use std::time::Duration;
use tempfile::tempdir;

/// The agent-control is configured with on agent with local configuration and a remote configuration was also set for the
/// corresponding sub-agent. After this, the configuration is set as empty which should fall-back to local
#[cfg(unix)]
#[test]
fn onhost_opamp_sub_agent_set_empty_config_defaults_to_local() {
    // Given a agent-control with a custom-agent running a sleep command with opamp configured.
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
    let _agent_control = start_agent_control_with_custom_config(base_paths.clone());

    let sub_agent_instance_id = get_instance_id(&AgentID::new(agent_id).unwrap(), base_paths);

    retry(60, Duration::from_secs(1), || {
        {
            // Then the retrieved effective config should match the expected remote config
            check_latest_effective_config_is_expected(
                &opamp_server,
                &sub_agent_instance_id.clone(),
                remote_values_config.to_string(),
            )
        }
    });

    // When the config is remotely set as empty, it should fall back to local
    opamp_server.set_config_response(sub_agent_instance_id.clone(), ConfigResponse::from(""));

    retry(60, Duration::from_secs(1), || {
        {
            // The retrieved effective config should match the expected local config
            check_latest_effective_config_is_expected(
                &opamp_server,
                &sub_agent_instance_id.clone(),
                local_values_config.to_string(),
            )
        }
    });
}

/// The agent-control is configured with on agent with local configuration but there is no local configuration
/// for the sub-agent. The corresponding sub-agent supervisor will not start until a remote configuration is received.
#[cfg(unix)]
#[test]
fn onhost_opamp_sub_agent_with_no_local_config() {
    // Given a agent-control with a custom-agent with opamp configured.

    use opamp_client::opamp::proto::RemoteConfigStatuses;
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

    let agent_id = "nr-sleep-agent";
    create_agent_control_config(
        opamp_server.endpoint(),
        agents.to_string(),
        local_dir.path().to_path_buf(),
        opamp_server.cert_file_path(),
    );

    // There is no local configuration for the sub-agent

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };
    std::thread::sleep(std::time::Duration::from_secs(1));
    let _agent_control = start_agent_control_with_custom_config(base_paths.clone());

    let sub_agent_instance_id = get_instance_id(&AgentID::new(agent_id).unwrap(), base_paths);

    // The supervisor will not start but the agent will be able to receive remote configurations
    retry(60, Duration::from_secs(1), || {
        {
            // The agent attributes should be informed even if there is no supervisor
            let _ = opamp_server
                .get_attributes(&sub_agent_instance_id.clone())
                .ok_or("no attributes informed")?;
            Ok(())
        }
    });

    // When the config is remotely set as empty, it should fall back to local
    let remote_values_config = "fake_variable: from-remote\n";
    opamp_server.set_config_response(
        sub_agent_instance_id.clone(),
        ConfigResponse::from(remote_values_config),
    );

    retry(60, Duration::from_secs(1), || {
        {
            if let Some(status) =
                opamp_server.get_remote_config_status(sub_agent_instance_id.clone())
            {
                if matches!(status.status(), RemoteConfigStatuses::Failed) {
                    panic!("Remote config for the sub-agent should not fail");
                }
            }
            // The retrieved effective config should match the expected local config
            check_latest_effective_config_is_expected(
                &opamp_server,
                &sub_agent_instance_id,
                remote_values_config.to_string(),
            )?;

            Ok(())
        }
    });
}
