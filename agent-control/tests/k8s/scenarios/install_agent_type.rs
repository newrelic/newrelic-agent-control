#![cfg(target_family = "unix")]
use crate::common::agent_control::{StartedAgentControl, start_agent_control_with_custom_config};
use crate::common::attributes::{
    check_identifying_attributes_contains_expected, convert_to_vec_key_value,
};
use crate::common::retry::{retry, retry_never};
use crate::common::runtime::{block_on, tokio_runtime};
use crate::k8s::tools::agent_control::{
    DYNAMIC_AGENT_TYPE_FILENAME, K8S_KEY_SECRET, K8S_PRIVATE_KEY_SECRET, TEST_CLUSTER_NAME,
    create_config_map,
};
use crate::k8s::tools::k8s_api::create_values_secret;
use crate::k8s::tools::{instance_id, k8s_env::K8sEnv};
use crate::on_host::tools::oci_package_manager::TestDataHelper;
use fake_opamp_server::FakeServer;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::defaults::OPAMP_SERVICE_VERSION;
use newrelic_agent_control::agent_control::defaults::{
    AGENT_CONTROL_ID, FOLDER_NAME_LOCAL_DATA, STORE_KEY_LOCAL_DATA_CONFIG,
};
use newrelic_agent_control::agent_control::run::BasePaths;
use newrelic_agent_control::agent_control::run::k8s::AGENT_CONTROL_MODE_K8S;
use newrelic_agent_control::agent_control::run::on_host::OCI_TEST_REGISTRY_URL;
use newrelic_agent_control::agent_type::agent_type_id::AgentTypeID;
use newrelic_agent_control::agent_type::oci::AgentTypeTag;
use newrelic_agent_control::environment::Environment;
use newrelic_agent_control::k8s::configmap_store::ConfigMapStore;
use newrelic_agent_control::on_host::file_store::build_config_name;
use oci_test_utils::{AgentTypeArtifact, OCISigner, PackagePublisher};
use opamp_client::opamp::proto::any_value::Value;
use std::io::Write as _;
use std::path::Path;
use std::time::Duration;
use tempfile::tempdir;

const AGENT_ID: &str = "test-agent-id";
const AGENT_TYPE_NAME: &str = "some.agent.type";
const AGENT_TYPE_NAMESPACE: &str = "k8s_install_agent_type";
const AGENT_TYPE_VERSION: &str = "0.1.0";

#[test]
#[ignore = "needs oci registry and a k8s cluster (use *with_oci_registry suffix)"]
fn k8s_test_local_agent_type_shadows_remote_registry_with_oci_registry() {
    let signer = OCISigner::start(tokio_runtime().handle().clone());

    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());
    let tmp_dir = tempdir().expect("failed to create local temp dir");

    write_agent_type_to_local_dir(tmp_dir.path());

    let mut opamp_server = FakeServer::start(tokio_runtime().handle());
    let mut agent_control = start_agent_control_for_test(
        &opamp_server,
        &signer,
        &namespace,
        k8s.client.clone(),
        tmp_dir.path(),
        "broken-url",
    );

    assert_sub_agent_reports_version(&mut opamp_server, k8s.client.clone(), &namespace);
    assert_agent_control_still_running(&mut agent_control);
}

#[test]
#[ignore = "needs oci registry and a k8s cluster (use *with_oci_registry suffix)"]
fn k8s_test_local_miss_resolves_via_remote_registry_with_oci_registry() {
    let signer = OCISigner::start(tokio_runtime().handle().clone());

    let mut k8s = block_on(K8sEnv::new());
    let namespace = block_on(k8s.test_namespace());
    let tmp_dir = tempdir().expect("failed to create local temp dir");

    push_agent_type_to_registry(&signer);

    let mut opamp_server = FakeServer::start(tokio_runtime().handle());
    let mut agent_control = start_agent_control_for_test(
        &opamp_server,
        &signer,
        &namespace,
        k8s.client.clone(),
        tmp_dir.path(),
        OCI_TEST_REGISTRY_URL,
    );

    assert_sub_agent_reports_version(&mut opamp_server, k8s.client.clone(), &namespace);
    assert_agent_control_still_running(&mut agent_control);
}

fn start_agent_control_for_test(
    opamp_server: &FakeServer,
    signer: &OCISigner,
    namespace: &str,
    k8s_client: kube::Client,
    local_dir: &Path,
    oci_registry: &str,
) -> StartedAgentControl {
    let config = format!(
        r#"fleet_control:
  endpoint: {}
  poll_interval: 5s
  signature_validation:
    public_key_server_url: {}
k8s:
  namespace: {namespace}
  namespace_agents: {namespace}
  cluster_name: {TEST_CLUSTER_NAME}
  auth_secret:
    secret_name: {K8S_PRIVATE_KEY_SECRET}
    secret_key_name: {K8S_KEY_SECRET}
oci:
  registry: "{oci_registry}"
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

    block_on(create_config_map(
        k8s_client.clone(),
        namespace,
        ConfigMapStore::build_cm_name(&AgentID::AgentControl, FOLDER_NAME_LOCAL_DATA).as_str(),
        config.clone(),
    ));

    let local = local_dir
        .join(FOLDER_NAME_LOCAL_DATA)
        .join(AGENT_CONTROL_ID);
    std::fs::create_dir_all(&local).unwrap();
    std::fs::File::create(local.join(build_config_name(STORE_KEY_LOCAL_DATA_CONFIG)))
        .unwrap()
        .write_all(config.as_bytes())
        .unwrap();

    create_values_secret(
        k8s_client,
        namespace,
        K8S_PRIVATE_KEY_SECRET,
        K8S_KEY_SECRET,
        crate::k8s::tools::agent_control::DUMMY_PRIVATE_KEY.to_string(),
    );

    start_agent_control_with_custom_config(
        BasePaths {
            local_dir: local_dir.to_path_buf(),
            remote_dir: local_dir.join("remote").to_path_buf(),
            log_dir: local_dir.join("log").to_path_buf(),
        },
        AGENT_CONTROL_MODE_K8S,
    )
}

fn push_agent_type_to_registry(signer: &OCISigner) -> oci_client::Reference {
    let tag = agent_type_tag(AGENT_TYPE_VERSION);

    let source_dir = tempdir().unwrap();
    let archive_dir = tempdir().unwrap();
    let archive = archive_dir.path().join("agent-type.tar.gz");
    TestDataHelper::compress_tar_gz(
        source_dir.path(),
        &archive,
        &agent_type_definition_yaml(AGENT_TYPE_VERSION),
        &format!("{tag}.yaml"),
    );

    let reference = PackagePublisher::new(tokio_runtime().handle().clone(), OCI_TEST_REGISTRY_URL)
        .push_with_tag(&archive, AgentTypeArtifact, tag.as_str());

    signer.sign_artifact(&reference);
    reference
}

fn write_agent_type_to_local_dir(local_dir: &Path) {
    let agent_type_file_path = local_dir.join(DYNAMIC_AGENT_TYPE_FILENAME);
    std::fs::create_dir_all(agent_type_file_path.parent().unwrap()).unwrap();
    std::fs::write(
        agent_type_file_path,
        agent_type_definition_yaml(AGENT_TYPE_VERSION),
    )
    .unwrap();
}

fn assert_sub_agent_reports_version(
    opamp_server: &mut FakeServer,
    k8s_client: kube::Client,
    namespace: &str,
) {
    let ac_instance_id =
        instance_id::get_instance_id(k8s_client.clone(), namespace, &AgentID::AgentControl);
    let agent_type_id = agent_type_id(AGENT_TYPE_VERSION);

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

    // Wait for the sub-agent's fleet-data ConfigMap to appear, which is written during
    // K8sSubAgentBuilder::build. This confirms AC received the config and successfully
    // resolved the agent type (either from local dir or remote OCI registry).
    let sub_agent_id = AgentID::try_from(AGENT_ID).unwrap();
    let sub_agent_instance_id =
        instance_id::get_instance_id(k8s_client.clone(), namespace, &sub_agent_id);

    opamp_server.set_config_response(
        sub_agent_instance_id.clone(),
        "{new_config: true}".to_string(),
    );

    // On k8s, the sub-agent reports the agent type version via the `service.version` identifying
    // attribute (set in sub_agent_start_settings). There is no runtime version checker on k8s.
    retry(60, Duration::from_secs(1), || {
        check_identifying_attributes_contains_expected(
            opamp_server,
            &sub_agent_instance_id,
            convert_to_vec_key_value(vec![(
                OPAMP_SERVICE_VERSION,
                Value::StringValue(AGENT_TYPE_VERSION.to_string()),
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
    AgentTypeTag::new(&agent_type_id(version), Environment::K8s)
}

fn agent_type_definition_yaml(agent_type_version: &str) -> String {
    format!(
        r#"
namespace: {AGENT_TYPE_NAMESPACE}
name: {AGENT_TYPE_NAME}
version: {agent_type_version}
protocol_version: "1.0"
platform: kubernetes
deployment:
  objects: {{}}
"#
    )
}
