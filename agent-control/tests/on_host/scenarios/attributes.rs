use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::attributes::{
    check_latest_identifying_attributes_match_expected,
    check_latest_non_identifying_attributes_match_expected, convert_to_vec_key_value,
};
use crate::common::base_paths::TempBasePaths;
use crate::common::retry::retry;
use crate::common::runtime::tokio_runtime;
use crate::on_host::tools::config::{OnHostAgentControlConfigBuilder, create_local_config};
use crate::on_host::tools::custom_agent_type::CustomAgentType;
use crate::on_host::tools::instance_id::get_instance_id;
use crate::on_host::tools::oci_package_manager::push_test_package;
use fake_opamp_server::FakeServer;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::defaults::{
    AGENT_CONTROL_NAMESPACE, HOST_NAME_ATTRIBUTE_KEY, OPAMP_AGENT_VERSION_ATTRIBUTE_KEY,
    OPAMP_SERVICE_NAME, OPAMP_SERVICE_NAMESPACE, OPAMP_SERVICE_VERSION, OPAMP_SUPERVISOR_KEY,
    OS_ATTRIBUTE_KEY, OS_ATTRIBUTE_VALUE, PARENT_AGENT_ID_ATTRIBUTE_KEY,
};
use newrelic_agent_control::agent_control::run::on_host::{
    AGENT_CONTROL_MODE_ON_HOST, OCI_TEST_REGISTRY_URL,
};
use oci_test_utils::OCISigner;
use opamp_client::opamp::proto::any_value::Value;
use opamp_client::opamp::proto::any_value::Value::BytesValue;
use resource_detection::system::hostname::get_hostname;
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
    let dirs = TempBasePaths::default();

    let agents = format!(
        r#"
  {agent_id}:
    agent_type: "{DEFAULT_NAMESPACE}/{DEFAULT_NAME}:{DEFAULT_VERSION}"
"#
    );

    OnHostAgentControlConfigBuilder::new(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_agents(agents.to_string())
        .write(dirs.local_dir());

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    let ac_instance_id = get_instance_id(&AgentID::AgentControl, dirs.base_paths());

    let test_agent_instance_id =
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
            BytesValue(ac_instance_id.clone().into()),
        ),
    ]));

    retry(30, Duration::from_secs(1), || {
        check_latest_identifying_attributes_match_expected(
            &opamp_server,
            &test_agent_instance_id,
            expected_identifying_attributes.clone(),
        )?;
        check_latest_non_identifying_attributes_match_expected(
            &opamp_server,
            &test_agent_instance_id,
            expected_non_identifying_attributes.clone(),
        )?;
        Ok(())
    });
}

/// Given an agent type that we know we are going to check if the default
/// identifying and non identifying attributes are what we expect plus
/// the "agent.version" related with the agent type.
#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix)"]
fn test_attributes_from_an_existing_agent_type_with_oci_registry() {
    let signer = OCISigner::start(tokio_runtime().handle().clone());
    let mut opamp_server = FakeServer::start(tokio_runtime().handle());
    let dirs = TempBasePaths::default();

    let version = "1.0.0";
    let agent_id = "attributes-test-agent";

    #[cfg(target_family = "unix")]
    let (script_name, script_content) = ("sleep.sh", "#!/bin/bash\nsleep 60\n");
    #[cfg(target_family = "windows")]
    let (script_name, script_content) = ("sleep.ps1", "Start-Sleep -Seconds 60\n");
    push_test_package(
        &signer,
        version,
        OCI_TEST_REGISTRY_URL,
        script_name,
        script_content,
    );

    let packages = format!(
        r#"
{agent_id}:
  type: tar
  download:
    oci:
      repository: test
      version: ${{nr-var:package_version}}
      public_key_url: {public_key_url}
"#,
        agent_id = agent_id,
        public_key_url = signer.jwks_url()
    );

    #[cfg(target_family = "unix")]
    let executables = {
        let script_path = format!("${{{{nr-sub:packages.{}.dir}}}}/sleep.sh", agent_id);
        format!(
            r#"[
        {{
            "id": "sleep-process",
            "path": "/bin/bash",
            "args": ["{}"]
        }}
    ]"#,
            script_path
        )
    };

    #[cfg(target_family = "windows")]
    let executables = {
        let script_path = format!("${{{{nr-sub:packages.{}.dir}}}}\\\\sleep.ps1", agent_id);
        format!(
            r#"[
        {{
            "id": "sleep-process",
            "path": "powershell.exe",
            "args": ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "{}"]
        }}
    ]"#,
            script_path
        )
    };

    let sleep_agent_type = CustomAgentType::default()
        .with_variables(
            r#"
package_version:
  description: "OCI package version to download"
  type: "string"
  required: false
  default: "latest"
"#,
        )
        .with_executables(Some(&executables))
        .with_packages(Some(&packages))
        .build(dirs.local_dir());

    OnHostAgentControlConfigBuilder::new(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_oci_registry(OCI_TEST_REGISTRY_URL)
        .write(dirs.local_dir());

    create_local_config(
        agent_id.to_string(),
        format!("package_version: '{}'", version),
        dirs.local_dir(),
    );

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);
    let ac_instance_id = get_instance_id(&AgentID::AgentControl, dirs.base_paths());

    let agents = format!(
        r#"
agents:
  {agent_id}:
    agent_type: "{sleep_agent_type}"
"#
    );
    opamp_server.set_config_response(ac_instance_id.clone(), agents);

    retry(60, Duration::from_secs(1), || {
        opamp_server
            .is_config_status_applied(ac_instance_id.clone())
            .map_err(|e| e.into())
    });

    let sleep_instance_id =
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
            Value::StringValue(version.to_string()),
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
            BytesValue(ac_instance_id.into()),
        ),
    ]));

    retry(30, Duration::from_secs(1), || {
        check_latest_identifying_attributes_match_expected(
            &opamp_server,
            &sleep_instance_id,
            expected_identifying_attributes.clone(),
        )?;
        check_latest_non_identifying_attributes_match_expected(
            &opamp_server,
            &sleep_instance_id,
            expected_non_identifying_attributes.clone(),
        )?;
        Ok(())
    })
}
