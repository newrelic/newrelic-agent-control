use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::attributes::{
    check_latest_identifying_attributes_match_expected,
    check_latest_non_identifying_attributes_match_expected, convert_to_vec_key_value,
};
use crate::common::retry::retry;
use crate::common::runtime::tokio_runtime;
use crate::on_host::consts::NO_CONFIG;
use crate::on_host::tools::base_paths::TempBasePaths;
use crate::on_host::tools::config::{AgentControlConfigBuilder, create_local_config};
use crate::on_host::tools::custom_agent_type::CustomAgentType;
use crate::on_host::tools::instance_id::get_instance_id;
use fake_opamp_server::FakeServer;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::defaults::{
    AGENT_CONTROL_NAMESPACE, HOST_NAME_ATTRIBUTE_KEY, OPAMP_AGENT_VERSION_ATTRIBUTE_KEY,
    OPAMP_SERVICE_NAME, OPAMP_SERVICE_NAMESPACE, OPAMP_SERVICE_VERSION, OPAMP_SUPERVISOR_KEY,
    OS_ATTRIBUTE_KEY, OS_ATTRIBUTE_VALUE, PARENT_AGENT_ID_ATTRIBUTE_KEY,
};
use newrelic_agent_control::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;
use opamp_client::opamp::proto::any_value::Value;
use opamp_client::opamp::proto::any_value::Value::BytesValue;
use resource_detection::system::hostname::get_hostname;
use rstest::rstest;
use std::path::PathBuf;
use std::time::Duration;

const DEFAULT_VERSION: &str = "0.3.0";
const DEFAULT_NAMESPACE: &str = "namespace";
const DEFAULT_NAME: &str = "name";

/// Given an agent type that we don't know we are going to check if the default
/// identifying and non identifying attributes are what we expect.
#[test]
fn test_attributes_from_non_existing_agent_type() {
    let opamp_server = FakeServer::start(tokio_runtime().handle());
    let agent_id = "test-agent";
    let dirs = TempBasePaths::new();

    let agents = format!(
        r#"
  {agent_id}:
    agent_type: "{DEFAULT_NAMESPACE}/{DEFAULT_NAME}:{DEFAULT_VERSION}"
"#
    );

    AgentControlConfigBuilder::basic(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_agents(agents.to_string())
        .write(dirs.local_dir());

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    let agent_control_instance_id_ac = get_instance_id(&AgentID::AgentControl, dirs.base_paths());

    let agent_control_instance_id =
        get_instance_id(&AgentID::try_from(agent_id).unwrap(), dirs.base_paths());

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
            OPAMP_SUPERVISOR_KEY,
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
            Value::StringValue(OS_ATTRIBUTE_VALUE.to_string()),
        ),
        (
            HOST_NAME_ATTRIBUTE_KEY,
            Value::StringValue(get_hostname().unwrap_or_default()),
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
#[cfg_attr(target_family = "unix", case::without_regex(|local_dir| {CustomAgentType::default().with_version(Some(r#"{"path": "echo", "args": ["-n","1.0.0"]}"#)).build(local_dir)}))]
#[cfg_attr(target_family = "windows", case::without_regex(|local_dir| {CustomAgentType::default().with_version(Some(r#"{"path": "cmd", "args": ["/C","set","/p=1.0.0<nul"]}"#)).build(local_dir)}))]
fn test_attributes_from_an_existing_agent_type(#[case] get_agent_type: impl Fn(PathBuf) -> String) {
    let opamp_server = FakeServer::start(tokio_runtime().handle());
    let dirs = TempBasePaths::new();

    // Add custom agent_type to registry
    let sleep_agent_type = get_agent_type(dirs.local_dir());
    let agent_id = "nr-sleep-agent";

    let agents = format!(
        r#"
agents:
  {agent_id}:
    agent_type: "{sleep_agent_type}"
"#
    );

    AgentControlConfigBuilder::basic(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_agents(agents.to_string())
        .write(dirs.local_dir());

    // And the custom-agent has empty config values
    create_local_config(
        agent_id.to_string(),
        NO_CONFIG.to_string(), // local empty config
        dirs.local_dir(),
    );

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);
    let agent_control_instance_id_ac = get_instance_id(&AgentID::AgentControl, dirs.base_paths());
    let agent_control_instance_id =
        get_instance_id(&AgentID::try_from(agent_id).unwrap(), dirs.base_paths());

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
            OPAMP_SUPERVISOR_KEY,
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
            Value::StringValue(OS_ATTRIBUTE_VALUE.to_string()),
        ),
        (
            HOST_NAME_ATTRIBUTE_KEY,
            Value::StringValue(get_hostname().unwrap_or_default()),
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
