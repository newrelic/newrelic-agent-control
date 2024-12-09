use crate::common::attributes::{
    check_latest_identifying_attributes_match_expected,
    check_latest_non_identifying_attributes_match_expected, get_expected_identifying_attributes,
    get_expected_non_identifying_attributes,
};
use crate::common::opamp::FakeServer;
use crate::common::retry::retry;
use crate::common::super_agent::start_super_agent_with_custom_config;
use crate::on_host::tools::config::create_super_agent_config;
use crate::on_host::tools::instance_id::get_instance_id;
use newrelic_super_agent::super_agent::config::AgentID;
use newrelic_super_agent::super_agent::defaults::FQN_NAME_INFRA_AGENT;
use newrelic_super_agent::super_agent::run::BasePaths;
use std::time::Duration;
use tempfile::tempdir;

const DEFAULT_VERSION: &str = "0.3.0";
const DEFAULT_NAMESPACE: &str = "namespace";
const DEFAULT_NAME: &str = "name";

/// Given an agent type that we don't know we are going to check if the default
/// identifying and non identifying attributes are what we expect.
#[cfg(unix)]
#[test]
fn test_attributes_from_non_existing_agent_type() {
    let opamp_server = FakeServer::start_new();

    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    let agents = format!(
        r#"
  test-agent:
    agent_type: "{}/{}:{}"
"#,
        DEFAULT_NAMESPACE, DEFAULT_NAME, DEFAULT_VERSION
    );

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

    let super_agent_instance_id_ac =
        get_instance_id(&AgentID::new_super_agent_id(), base_paths.clone());

    let super_agent_instance_id =
        get_instance_id(&AgentID::new("test-agent").unwrap(), base_paths.clone());

    let expected_identifying_attributes = get_expected_identifying_attributes(
        DEFAULT_NAMESPACE.to_string(),
        DEFAULT_NAME.to_string(),
        DEFAULT_VERSION.to_string(),
        None,
        None,
    );

    let expected_non_identifying_attributes =
        get_expected_non_identifying_attributes(super_agent_instance_id_ac);

    retry(30, Duration::from_secs(1), || {
        check_latest_identifying_attributes_match_expected(
            &opamp_server,
            &super_agent_instance_id,
            expected_identifying_attributes.clone(),
        )?;
        check_latest_non_identifying_attributes_match_expected(
            &opamp_server,
            &super_agent_instance_id,
            expected_non_identifying_attributes.clone(),
        )?;
        Ok(())
    });
}

/// Given an agent type that we know we are going to check if the default
/// identifying and non identifying attributes are what we expect plus
/// the "agent.version" related with the agent type.
#[cfg(unix)]
#[test]
fn test_attributes_from_an_existing_agent_type() {
    let opamp_server = FakeServer::start_new();
    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    let agents = format!(
        r#"
  test-agent:
    agent_type: "{}/{}:{}"
"#,
        DEFAULT_NAMESPACE, FQN_NAME_INFRA_AGENT, DEFAULT_VERSION
    );

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
    let super_agent_instance_id_ac =
        get_instance_id(&AgentID::new_super_agent_id(), base_paths.clone());
    let super_agent_instance_id =
        get_instance_id(&AgentID::new("test-agent").unwrap(), base_paths.clone());
    let expected_identifying_attributes = get_expected_identifying_attributes(
        DEFAULT_NAMESPACE.to_string(),
        FQN_NAME_INFRA_AGENT.to_string(),
        DEFAULT_VERSION.to_string(),
        Some("0.0.0".to_string()),
        None,
    );

    let expected_non_identifying_attributes =
        get_expected_non_identifying_attributes(super_agent_instance_id_ac);

    retry(30, Duration::from_secs(1), || {
        check_latest_identifying_attributes_match_expected(
            &opamp_server,
            &super_agent_instance_id,
            expected_identifying_attributes.clone(),
        )?;
        check_latest_non_identifying_attributes_match_expected(
            &opamp_server,
            &super_agent_instance_id,
            expected_non_identifying_attributes.clone(),
        )?;
        Ok(())
    })
}
