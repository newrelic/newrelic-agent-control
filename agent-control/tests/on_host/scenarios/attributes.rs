#![cfg(target_family = "unix")]
use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::attributes::{
    check_latest_identifying_attributes_match_expected,
    check_latest_non_identifying_attributes_match_expected, convert_to_vec_key_value,
};
use crate::common::opamp::FakeServer;
use crate::common::retry::retry;
use crate::on_host::consts::NO_CONFIG;
use crate::on_host::tools::config::{create_agent_control_config, create_local_config};
use crate::on_host::tools::custom_agent_type::CustomAgentType;
use crate::on_host::tools::instance_id::get_instance_id;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::defaults::{
    AGENT_CONTROL_NAMESPACE, HOST_NAME_ATTRIBUTE_KEY, OPAMP_AGENT_VERSION_ATTRIBUTE_KEY,
    OPAMP_SERVICE_INSTANCE_ID, OPAMP_SERVICE_NAME, OPAMP_SERVICE_NAMESPACE, OPAMP_SERVICE_VERSION,
    OS_ATTRIBUTE_KEY, OS_VERSION_ATTRIBUTE_VALUE, PARENT_AGENT_ID_ATTRIBUTE_KEY,
};
use newrelic_agent_control::agent_control::run::{BasePaths, Environment};
use nix::unistd::gethostname;
use opamp_client::opamp::proto::any_value::Value;
use opamp_client::opamp::proto::any_value::Value::BytesValue;
use rstest::rstest;
use std::path::PathBuf;
use std::time::Duration;
use tempfile::tempdir;

const DEFAULT_VERSION: &str = "0.3.0";
const DEFAULT_NAMESPACE: &str = "namespace";
const DEFAULT_NAME: &str = "name";

/// Given an agent type that we don't know we are going to check if the default
/// identifying and non identifying attributes are what we expect.
#[test]
fn test_attributes_from_non_existing_agent_type() {
    let opamp_server = FakeServer::start_new();
    let agent_id = "test-agent";
    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    let agents = format!(
        r#"
  {agent_id}:
    agent_type: "{DEFAULT_NAMESPACE}/{DEFAULT_NAME}:{DEFAULT_VERSION}"
"#
    );

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
    let _agent_control =
        start_agent_control_with_custom_config(base_paths.clone(), Environment::OnHost);

    let agent_control_instance_id_ac = get_instance_id(&AgentID::AgentControl, base_paths.clone());

    let agent_control_instance_id =
        get_instance_id(&AgentID::try_from(agent_id).unwrap(), base_paths.clone());

    let expected_identifying_attributes = convert_to_vec_key_value(Vec::from([
        (
            OPAMP_SERVICE_NAMESPACE,
            Value::StringValue(DEFAULT_NAMESPACE.to_string()),
        ),
        (
            OPAMP_SERVICE_NAME,
            Value::StringValue(DEFAULT_NAME.to_string()),
        ),
        (
            OPAMP_SERVICE_INSTANCE_ID,
            Value::StringValue(agent_id.to_string()),
        ),
        (
            OPAMP_SERVICE_VERSION,
            Value::StringValue(DEFAULT_VERSION.to_string()),
        ),
    ]));

    let expected_non_identifying_attributes = convert_to_vec_key_value(Vec::from([
        (
            OS_ATTRIBUTE_KEY,
            Value::StringValue(OS_VERSION_ATTRIBUTE_VALUE.to_string()),
        ),
        (
            HOST_NAME_ATTRIBUTE_KEY,
            Value::StringValue(gethostname().unwrap_or_default().into_string().unwrap()),
        ),
        (
            PARENT_AGENT_ID_ATTRIBUTE_KEY,
            BytesValue(agent_control_instance_id_ac.clone().into()),
        ),
    ]));

    retry(30, Duration::from_secs(1), || {
        check_latest_identifying_attributes_match_expected(
            &opamp_server,
            &agent_control_instance_id,
            expected_identifying_attributes.clone(),
        )?;
        check_latest_non_identifying_attributes_match_expected(
            &opamp_server,
            &agent_control_instance_id,
            expected_non_identifying_attributes.clone(),
        )?;
        Ok(())
    });
}

/// Given an agent type that we know we are going to check if the default
/// identifying and non identifying attributes are what we expect plus
/// the "agent.version" related with the agent type.
#[rstest]
#[case::with_regex(|local_dir| {CustomAgentType::default().build(local_dir)})]
#[case::without_regex(|local_dir| {CustomAgentType::default().with_version(Some(r#"{"path": "echo", "args": "-n 1.0.0"}"#)).build(local_dir)})]
fn test_attributes_from_an_existing_agent_type(#[case] get_agent_type: impl Fn(PathBuf) -> String) {
    let opamp_server = FakeServer::start_new();
    let local_dir = tempdir().expect("failed to create local temp dir");
    let remote_dir = tempdir().expect("failed to create remote temp dir");

    // Add custom agent_type to registry
    let sleep_agent_type = get_agent_type(local_dir.path().to_path_buf());
    let agent_id = "nr-sleep-agent";

    let agents = format!(
        r#"
agents:
  {agent_id}:
    agent_type: "{sleep_agent_type}"
"#
    );

    create_agent_control_config(
        opamp_server.endpoint(),
        opamp_server.jwks_endpoint(),
        agents.to_string(),
        local_dir.path().to_path_buf(),
    );

    // And the custom-agent has empty config values
    create_local_config(
        agent_id.to_string(),
        NO_CONFIG.to_string(), // local empty config
        local_dir.path().into(),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };

    let _agent_control =
        start_agent_control_with_custom_config(base_paths.clone(), Environment::OnHost);
    let agent_control_instance_id_ac = get_instance_id(&AgentID::AgentControl, base_paths.clone());
    let agent_control_instance_id =
        get_instance_id(&AgentID::try_from(agent_id).unwrap(), base_paths.clone());

    let expected_identifying_attributes = convert_to_vec_key_value(Vec::from([
        (
            OPAMP_SERVICE_NAMESPACE,
            Value::StringValue(AGENT_CONTROL_NAMESPACE.to_string()),
        ),
        (
            OPAMP_SERVICE_NAME,
            Value::StringValue("com.newrelic.custom_agent".to_string()),
        ),
        (
            OPAMP_SERVICE_VERSION,
            Value::StringValue("0.1.0".to_string()),
        ),
        (
            OPAMP_SERVICE_INSTANCE_ID,
            Value::StringValue(agent_id.to_string()),
        ),
        (
            OPAMP_AGENT_VERSION_ATTRIBUTE_KEY,
            Value::StringValue("1.0.0".to_string()),
        ),
    ]));

    let expected_non_identifying_attributes = convert_to_vec_key_value(Vec::from([
        (
            OS_ATTRIBUTE_KEY,
            Value::StringValue(OS_VERSION_ATTRIBUTE_VALUE.to_string()),
        ),
        (
            HOST_NAME_ATTRIBUTE_KEY,
            Value::StringValue(gethostname().unwrap_or_default().into_string().unwrap()),
        ),
        (
            PARENT_AGENT_ID_ATTRIBUTE_KEY,
            BytesValue(agent_control_instance_id_ac.into()),
        ),
    ]));

    retry(30, Duration::from_secs(1), || {
        check_latest_identifying_attributes_match_expected(
            &opamp_server,
            &agent_control_instance_id,
            expected_identifying_attributes.clone(),
        )?;
        check_latest_non_identifying_attributes_match_expected(
            &opamp_server,
            &agent_control_instance_id,
            expected_non_identifying_attributes.clone(),
        )?;
        Ok(())
    })
}
