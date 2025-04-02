use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::effective_config::check_latest_effective_config_is_expected;
use crate::common::opamp::ConfigResponse;
use crate::common::remote_config_status::check_latest_remote_config_status_is_expected;
use crate::common::{opamp::FakeServer, retry::retry};
use crate::on_host::tools::config::load_remote_config_content;
use crate::on_host::tools::config::{create_agent_control_config, create_sub_agent_values};
use crate::on_host::tools::custom_agent_type::get_agent_type_custom;
use crate::on_host::tools::instance_id::get_instance_id;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::defaults::{SUB_AGENT_DIR, VALUES_DIR, VALUES_FILENAME};
use newrelic_agent_control::agent_control::run::BasePaths;
use opamp_client::opamp::proto::RemoteConfigStatuses;
use std::time::Duration;
use tempfile::tempdir;

/// A agent control has a sub agent configured and the sub agent has a local configuration, then a **invalid** remote
/// configuration is set. This test checks:
/// - That the latest remote config status is failed.
/// - The failed remote config should not be persisted.
/// - That latest effective configuration reported is the local one (which is valid).
#[cfg(unix)]
#[test]
fn onhost_opamp_sub_agent_invalid_remote_config() {
    // Given a agent-control with a custom-agent running a sleep command with opamp configured.

    use newrelic_agent_control::agent_control::run::Environment;

    let mut opamp_server = FakeServer::start_new();

    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    let sub_agent_id = AgentID::new("nr-sleep-agent").unwrap();

    let sleep_agent_type = get_agent_type_custom(
        local_dir.path().to_path_buf(),
        "sh",
        "tests/on_host/data/trap_term_sleep_60.sh",
    );
    let agents = format!(
        r#"
  {sub_agent_id}:
    agent_type: "{sleep_agent_type}"
"#,
    );

    create_agent_control_config(
        opamp_server.endpoint(),
        agents.to_string(),
        local_dir.path().to_path_buf(),
        opamp_server.cert_file_path(),
    );

    // And the custom-agent has local config values

    let local_config = "fake_variable: from local\n";
    create_sub_agent_values(
        sub_agent_id.to_string(),
        local_config.to_string(),
        local_dir.path().to_path_buf(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };

    let _agent_control =
        start_agent_control_with_custom_config(base_paths.clone(), Environment::OnHost);

    let sub_agent_instance_id = get_instance_id(&sub_agent_id, base_paths.clone());

    // When a new incorrect config is received from OpAMP
    opamp_server.set_config_response(
        sub_agent_instance_id.clone(),
        // The configuration is invalid since a string is expected
        ConfigResponse::from("fake_variable: 123"),
    );

    retry(60, Duration::from_secs(1), || {
        {
            // Then the remote config should not be created in the remote filesystem.
            if load_remote_config_content(&sub_agent_id, base_paths.clone()).is_some() {
                return Err("Remote config file should not be created".into());
            }

            check_latest_remote_config_status_is_expected(
                &opamp_server,
                &sub_agent_instance_id,
                RemoteConfigStatuses::Failed as i32,
            )?;

            // And effective_config should return the initial local one
            check_latest_effective_config_is_expected(
                &opamp_server,
                &sub_agent_instance_id,
                local_config.to_string(),
            )
        }
    });
}

/// A agent control has a sub agent (without executables supervisor) configured and the sub agent has a local configuration, then a **invalid** remote
/// configuration is set. This test checks:
/// - Same as `onhost_opamp_sub_agent_invalid_remote_config` test but without executables supervisor.
/// - That the latest remote config status is failed.
/// - The failed remote config should not be persisted.
/// - That latest effective configuration reported is the local one (which is valid).
#[cfg(unix)]
#[test]
fn test_invalid_config_executalbe_less_supervisor() {
    use newrelic_agent_control::agent_control::run::Environment;

    use crate::on_host::tools::custom_agent_type::get_agent_type_without_deployment;

    let mut opamp_server = FakeServer::start_new();

    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");
    let sub_agent_id = AgentID::new("test-agent").unwrap();

    let agent_type = get_agent_type_without_deployment(local_dir.path().to_path_buf());

    let agents = format!(
        r#"
  {sub_agent_id}:
    agent_type: "{agent_type}"
"#
    );

    let local_config = "fake_variable: from local\n";
    create_sub_agent_values(
        sub_agent_id.to_string(),
        local_config.to_string(),
        local_dir.path().to_path_buf(),
    );

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

    let sub_agent_instance_id = get_instance_id(&sub_agent_id, base_paths.clone());

    // When a new incorrect config is received from OpAMP
    opamp_server.set_config_response(
        sub_agent_instance_id.clone(),
        // The configuration is invalid since a string is expected
        ConfigResponse::from("fake_variable: 123"),
    );

    retry(60, Duration::from_secs(1), || {
        {
            // Then the remote config should be created in the remote filesystem.
            let remote_file = remote_dir
                .path()
                .join(SUB_AGENT_DIR)
                .join(sub_agent_id.clone())
                .join(VALUES_DIR)
                .join(VALUES_FILENAME);
            if remote_file.exists() {
                return Err("Remote config file should not be created".into());
            }

            check_latest_remote_config_status_is_expected(
                &opamp_server,
                &sub_agent_instance_id,
                RemoteConfigStatuses::Failed as i32,
            )?;

            // And effective_config should return the initial local one
            check_latest_effective_config_is_expected(
                &opamp_server,
                &sub_agent_instance_id,
                local_config.to_string(),
            )
        }
    });
}

/// A agent control has a sub agent configured and the sub agent has a local configuration, and applied remote config, then a **invalid** remote
/// configuration is set. This test checks:
/// - That the latest remote config status is failed.
/// - The failed remote config should not be persisted.
/// - That latest effective configuration reported is the latest applied valid remote config.
#[cfg(unix)]
#[test]
fn onhost_opamp_sub_agent_invalid_remote_config_rollback_previous_remote() {
    // Given a agent-control with a custom-agent running a sleep command with opamp configured.

    use newrelic_agent_control::agent_control::run::Environment;

    let mut opamp_server = FakeServer::start_new();

    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    let sub_agent_id = AgentID::new("nr-sleep-agent").unwrap();

    let sleep_agent_type = get_agent_type_custom(
        local_dir.path().to_path_buf(),
        "sh",
        "tests/on_host/data/trap_term_sleep_60.sh",
    );
    let agents = format!(
        r#"
  {sub_agent_id}:
    agent_type: "{sleep_agent_type}"
"#,
    );

    create_agent_control_config(
        opamp_server.endpoint(),
        agents.to_string(),
        local_dir.path().to_path_buf(),
        opamp_server.cert_file_path(),
    );

    // And the custom-agent has local config values

    let local_config = "fake_variable: from local\n";
    create_sub_agent_values(
        sub_agent_id.to_string(),
        local_config.to_string(),
        local_dir.path().to_path_buf(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };

    let _agent_control =
        start_agent_control_with_custom_config(base_paths.clone(), Environment::OnHost);

    let sub_agent_instance_id = get_instance_id(&sub_agent_id, base_paths.clone());

    let valid_remote_config = "fake_variable: valid from remote\n";
    opamp_server.set_config_response(
        sub_agent_instance_id.clone(),
        ConfigResponse::from(valid_remote_config),
    );

    // Verify valid remote config was applied
    retry(60, Duration::from_secs(1), || {
        check_latest_effective_config_is_expected(
            &opamp_server,
            &sub_agent_instance_id,
            valid_remote_config.to_string(),
        )
    });

    // Send invalid remote config
    opamp_server.set_config_response(
        sub_agent_instance_id.clone(),
        // fake_variable expects string
        ConfigResponse::from("fake_variable: 123\n"),
    );
    // Verify valid remote config was applied
    retry(60, Duration::from_secs(1), || {
        // Then the remote config should be created in the remote filesystem.
        let Some(actual_remote_config) =
            load_remote_config_content(&sub_agent_id, base_paths.clone())
        else {
            return Err("Persisted remote config should exist from previous step".into());
        };

        if actual_remote_config != valid_remote_config {
            return Err("Persisted remote config should be the latest valid".into());
        }

        check_latest_remote_config_status_is_expected(
            &opamp_server,
            &sub_agent_instance_id,
            RemoteConfigStatuses::Failed as i32,
        )?;

        // And effective_config should return the valid remote one
        check_latest_effective_config_is_expected(
            &opamp_server,
            &sub_agent_instance_id,
            valid_remote_config.to_string(),
        )
    });
}
