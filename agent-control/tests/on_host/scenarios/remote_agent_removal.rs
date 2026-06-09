use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::effective_config::check_latest_effective_config_is_expected;
use crate::common::remote_config_status::check_latest_remote_config_status_is_expected;
use crate::common::{retry::retry, runtime::tokio_runtime};
use crate::on_host::tools::config::create_agent_control_config;
use crate::on_host::tools::custom_agent_type::CustomAgentType;
use crate::on_host::tools::instance_id::get_instance_id;
use fake_opamp_server::FakeServer;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::defaults::AGENT_FILESYSTEM_FOLDER_NAME;
use newrelic_agent_control::agent_control::run::BasePaths;
use newrelic_agent_control::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;
use opamp_client::opamp::proto::RemoteConfigStatuses;
use std::time::Duration;
use tempfile::tempdir;

/// Given
/// 1. AC has no agents
/// 2. One sub-agent is remotely added
/// 3. The sub-agent is remotely removed
/// 4. The sub-agent is remotely added back
#[test]
fn onhost_opamp_agent_control_remote_config_add_remove_add_agent() {
    // Given a agent-control without agents and opamp configured.

    let mut opamp_server = FakeServer::start(tokio_runtime().handle());

    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    let agents = "{}";
    create_agent_control_config(
        opamp_server.endpoint(),
        opamp_server.jwks_endpoint(),
        agents.to_string(),
        local_dir.path().to_path_buf(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };

    let dir_entry = "test-dir";
    let file_path = "test-file.txt";
    let content_template = "${nr-var:file_contents}";
    let first_templated_content = "first";
    let second_templated_content = "second";

    // Add custom agent_type to registry with filesystem operations
    let sleep_agent_type = CustomAgentType::default()
        .with_filesystem(Some(&format!(
            r#"
{dir_entry}:
  {file_path}: "{content_template}"
"#
        )))
        .with_variables(
            r#"
file_contents:
  description: "Contents of the file"
  type: "string"
  required: true
"#,
        )
        .build(local_dir.path().to_path_buf());

    let _agent_control =
        start_agent_control_with_custom_config(base_paths.clone(), AGENT_CONTROL_MODE_ON_HOST);

    let ac_instance_id = get_instance_id(&AgentID::AgentControl, base_paths.clone());

    let agent_id = "nr-sleep-agent";
    let agents_config_with_agent = format!(
        r#"
agents:
  {agent_id}:
    agent_type: "{sleep_agent_type}"
"#
    );
    let agents_config_empty = "agents: {}";

    // 1. Add agent
    opamp_server.set_config_response(ac_instance_id.clone(), agents_config_with_agent.as_str());

    // Wait for AC to apply configuration
    retry(60, Duration::from_secs(1), || {
        check_latest_remote_config_status_is_expected(
            &opamp_server,
            &ac_instance_id,
            RemoteConfigStatuses::Applied as i32,
        )
    });

    let subagent_instance_id =
        get_instance_id(&AgentID::try_from(agent_id).unwrap(), base_paths.clone());

    // Provide config for the subagent
    opamp_server.set_config_response(
        subagent_instance_id.clone(),
        format!("file_contents: {}", first_templated_content),
    );

    // Wait for subagent to process the remote config
    retry(60, Duration::from_secs(1), || {
        check_latest_remote_config_status_is_expected(
            &opamp_server,
            &subagent_instance_id,
            RemoteConfigStatuses::Applied as i32,
        )
    });

    let agent_filesystem_path = base_paths
        .remote_dir
        .join(AGENT_FILESYSTEM_FOLDER_NAME)
        .join(agent_id)
        .join(dir_entry)
        .join(file_path);

    // Check that the file was created
    retry(60, Duration::from_secs(1), || {
        if !agent_filesystem_path.exists() {
            return Err(format!("File not found at {:?}", agent_filesystem_path).into());
        }
        let content = std::fs::read_to_string(&agent_filesystem_path)?;
        if content != first_templated_content {
            return Err(format!(
                "Content mismatch: expected {}, got {}",
                first_templated_content, content
            )
            .into());
        }
        Ok(())
    });
    retry(60, Duration::from_secs(1), || {
        check_latest_effective_config_is_expected(
            &opamp_server,
            &subagent_instance_id,
            format!("file_contents: {}\n", first_templated_content),
        )
    });

    // 2. Remove agent
    opamp_server.set_config_response(ac_instance_id.clone(), agents_config_empty);

    // Verify agent control updates its effective config to empty
    // This confirms the removal was processed
    retry(60, Duration::from_secs(1), || {
        check_latest_effective_config_is_expected(
            &opamp_server,
            &ac_instance_id,
            "agents: {}\n".to_string(),
        )
    });

    // 3. Add agent again
    opamp_server.set_config_response(ac_instance_id.clone(), agents_config_with_agent.as_str());

    let new_subagent_instance_id =
        get_instance_id(&AgentID::try_from(agent_id).unwrap(), base_paths.clone());

    assert_ne!(
        new_subagent_instance_id, subagent_instance_id,
        "identifier should have been recreated"
    );

    // Previous remote config should have been removed
    retry(60, Duration::from_secs(1), || {
        check_latest_effective_config_is_expected(
            &opamp_server,
            &new_subagent_instance_id,
            "".to_string(),
        )
    });

    // 4. Add a remote config for the agent with updated values so it refreshes the filesystem
    opamp_server.set_config_response(
        new_subagent_instance_id.clone(),
        format!("file_contents: {}", second_templated_content),
    );

    // Check that the file has correct content
    retry(60, Duration::from_secs(1), || {
        if !agent_filesystem_path.exists() {
            return Err(format!("File not found at {:?}", agent_filesystem_path).into());
        }
        let content = std::fs::read_to_string(&agent_filesystem_path)?;
        if content != second_templated_content {
            return Err(format!(
                "Content mismatch: expected {}, got {}",
                second_templated_content, content
            )
            .into());
        }
        Ok(())
    });
}
