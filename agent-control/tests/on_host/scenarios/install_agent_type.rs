use crate::common::agent_control::{StartedAgentControl, start_agent_control_with_custom_config};
use crate::common::attributes::{
    check_identifying_attributes_contains_expected, convert_to_vec_key_value,
};
use crate::common::retry::{retry, retry_never};
use crate::common::runtime::tokio_runtime;
use crate::on_host::tools::config::create_local_config;
use crate::on_host::tools::instance_id::get_instance_id;
use crate::on_host::tools::oci_package_manager::TestDataHelper;
use fake_opamp_server::FakeServer;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::defaults::{
    AGENT_CONTROL_ID, DYNAMIC_AGENT_TYPES_DIR, OPAMP_AGENT_VERSION_ATTRIBUTE_KEY,
};
use newrelic_agent_control::agent_control::run::BasePaths;
use newrelic_agent_control::agent_control::run::on_host::{
    AGENT_CONTROL_MODE_ON_HOST, OCI_TEST_REGISTRY_URL,
};
use newrelic_agent_control::agent_type::agent_type_id::AgentTypeID;
use newrelic_agent_control::agent_type::oci::AgentTypeTag;
use oci_test_utils::{AgentTypeArtifact, OCISigner, PackagePublisher};
use opamp_client::opamp::proto::any_value::Value;
use std::path::Path;
use std::time::Duration;
use tempfile::tempdir;

const AGENT_ID: &str = "test-agent-id";
const AGENT_TYPE_NAME: &str = "some.agent.type";
const AGENT_TYPE_NAMESPACE: &str = "install_agent_type";

const LOCAL_VERSION_CHECKER_OUTPUT: &str = "1.0.0";
const REMOTE_VERSION_CHECKER_OUTPUT: &str = "2.0.0";

#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix)"]
fn test_local_miss_resolves_via_remote_registry_with_oci_registry() {
    let signer = OCISigner::start(tokio_runtime().handle().clone());
    // Each test uses a different agent type version to avoid concurrency issues.
    let agent_type_version = "0.1.0";

    push_agent_type_to_registry(&signer, agent_type_version, REMOTE_VERSION_CHECKER_OUTPUT);

    let mut opamp_server = FakeServer::start(tokio_runtime().handle());
    let base_paths = temp_base_paths();
    let mut agent_control = start_agent_control_for_test(&opamp_server, &signer, &base_paths);

    assert_sub_agent_reports_version(
        &mut opamp_server,
        &base_paths,
        agent_type_version,
        REMOTE_VERSION_CHECKER_OUTPUT,
    );
    assert_agent_control_still_running(&mut agent_control);
}

#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix)"]
fn test_local_agent_type_shadows_remote_registry_with_oci_registry() {
    let signer = OCISigner::start(tokio_runtime().handle().clone());
    // Each test uses a different agent type version to avoid concurrency issues.
    let agent_type_version = "0.2.0";

    // Push the remote variant — version checker will output REMOTE_VERSION_CHECKER_OUTPUT.
    push_agent_type_to_registry(&signer, agent_type_version, REMOTE_VERSION_CHECKER_OUTPUT);

    // Write the local variant — version checker will output LOCAL_VERSION_CHECKER_OUTPUT.
    let base_paths = temp_base_paths();
    write_agent_type_to_local_dir(
        &base_paths.local_dir,
        agent_type_version,
        LOCAL_VERSION_CHECKER_OUTPUT,
    );

    let mut opamp_server = FakeServer::start(tokio_runtime().handle());
    let mut agent_control = start_agent_control_for_test(&opamp_server, &signer, &base_paths);

    assert_sub_agent_reports_version(
        &mut opamp_server,
        &base_paths,
        agent_type_version,
        LOCAL_VERSION_CHECKER_OUTPUT,
    );

    assert_agent_control_still_running(&mut agent_control);
}

fn temp_base_paths() -> BasePaths {
    let local_dir = tempdir().unwrap();
    let remote_dir = tempdir().unwrap();

    BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    }
}

fn start_agent_control_for_test(
    opamp_server: &FakeServer,
    signer: &OCISigner,
    base_paths: &BasePaths,
) -> StartedAgentControl {
    let config = format!(
        r#"
host_id: integration-test
fleet_control:
  endpoint: {}
  poll_interval: 5s
  signature_validation:
    public_key_server_url: {}
oci:
  registry: "{OCI_TEST_REGISTRY_URL}"
agent_types:
  default_remote:
    repository: test
    signature_verification_enabled: true
    public_key_url: {}
agents: {{}}
"#,
        opamp_server.endpoint(),
        opamp_server.jwks_endpoint(),
        signer.jwks_url(),
    );
    create_local_config(AGENT_CONTROL_ID, config, base_paths.local_dir.clone());

    start_agent_control_with_custom_config(base_paths.clone(), AGENT_CONTROL_MODE_ON_HOST)
}

fn push_agent_type_to_registry(
    signer: &OCISigner,
    agent_type_version: &str,
    reported_version: &str,
) -> oci_client::Reference {
    let tag = agent_type_tag(agent_type_version);

    let source_dir = tempdir().unwrap();
    let archive_dir = tempdir().unwrap();
    let archive = archive_dir.path().join("agent-type.tar.gz");
    TestDataHelper::compress_tar_gz(
        source_dir.path(),
        &archive,
        &agent_type_definition_yaml(agent_type_version, reported_version),
        &format!("{tag}.yaml"),
    );

    let reference = PackagePublisher::new(tokio_runtime().handle().clone(), OCI_TEST_REGISTRY_URL)
        .push_with_tag(&archive, AgentTypeArtifact, tag.as_str());

    signer.sign_artifact(&reference);
    reference
}

fn write_agent_type_to_local_dir(
    local_dir: &Path,
    agent_type_version: &str,
    reported_version: &str,
) {
    let dynamic_dir = local_dir.join(DYNAMIC_AGENT_TYPES_DIR);
    std::fs::create_dir_all(&dynamic_dir).unwrap();
    std::fs::write(
        dynamic_dir.join("type.yaml"),
        agent_type_definition_yaml(agent_type_version, reported_version),
    )
    .unwrap();
}

fn assert_sub_agent_reports_version(
    opamp_server: &mut FakeServer,
    base_paths: &BasePaths,
    agent_type_version: &str,
    expected_version: &str,
) {
    let ac_instance_id = get_instance_id(&AgentID::AgentControl, base_paths.clone());
    let agent_type_id = agent_type_id(agent_type_version);

    opamp_server.set_config_response(
        ac_instance_id.clone(),
        format!(
            r#"
agents:
  {AGENT_ID}:
    agent_type: "{agent_type_id}"
"#
        ),
    );

    retry(60, Duration::from_secs(1), || {
        opamp_server
            .is_config_status_applied(ac_instance_id.clone())
            .map_err(|e| e.into())
    });

    let sub_agent_id = AgentID::try_from(AGENT_ID).unwrap();
    let sub_agent_instance_id = get_instance_id(&sub_agent_id, base_paths.clone());

    opamp_server.set_config_response(
        sub_agent_instance_id.clone(),
        "{new_config: true}".to_string(),
    );

    retry(60, Duration::from_secs(1), || {
        check_identifying_attributes_contains_expected(
            opamp_server,
            &sub_agent_instance_id,
            convert_to_vec_key_value(vec![(
                OPAMP_AGENT_VERSION_ATTRIBUTE_KEY,
                Value::StringValue(expected_version.to_string()),
            )]),
        )
        .map_err(|e| e.into())
    });
}

fn assert_agent_control_still_running(agent_control: &mut StartedAgentControl) {
    retry_never(10, Duration::from_secs(1), || {
        if agent_control.has_gracefully_stopped() {
            Err("agent-control stopped unexpectedly".into())
        } else {
            Ok(())
        }
    });
}

fn agent_type_id(version: &str) -> AgentTypeID {
    let id = format!("{AGENT_TYPE_NAMESPACE}/{AGENT_TYPE_NAME}:{version}");
    AgentTypeID::try_from(id.as_str()).unwrap()
}

fn agent_type_tag(version: &str) -> AgentTypeTag {
    AgentTypeTag::new(&agent_type_id(version), AGENT_CONTROL_MODE_ON_HOST)
}

#[cfg(not(target_os = "windows"))]
fn agent_type_definition_yaml(agent_type_version: &str, reported_version: &str) -> String {
    format!(
        r#"
namespace: {AGENT_TYPE_NAMESPACE}
name: {AGENT_TYPE_NAME}
version: {agent_type_version}
protocol_version: "1.0"
platform: host
operating_system: linux
deployment:
  version:
    path: echo
    args: ["{reported_version}"]
    regex: \d+\.\d+\.\d+
"#
    )
}

#[cfg(target_os = "windows")]
fn agent_type_definition_yaml(agent_type_version: &str, reported_version: &str) -> String {
    format!(
        r#"
namespace: {AGENT_TYPE_NAMESPACE}
name: {AGENT_TYPE_NAME}
version: {agent_type_version}
protocol_version: "1.0"
platform: host
operating_system: windows
deployment:
  version:
    path: cmd
    args: ["/C", "echo", "{reported_version}"]
    regex: \d+\.\d+\.\d+
"#
    )
}
