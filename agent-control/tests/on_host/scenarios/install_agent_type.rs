use crate::common::agent_control::{StartedAgentControl, start_agent_control_with_custom_config};
use crate::common::attributes::{
    check_identifying_attributes_contains_expected, convert_to_vec_key_value,
};
use crate::common::base_paths::TempBasePaths;
use crate::common::retry::{retry, retry_never};
use crate::common::runtime::tokio_runtime;
use crate::on_host::tools::config::OnHostAgentControlConfigBuilder;
use crate::on_host::tools::instance_id::get_instance_id;
use crate::on_host::tools::oci_package_manager::{TestDataHelper, push_test_package};
use fake_opamp_server::FakeServer;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::defaults::{
    DYNAMIC_AGENT_TYPES_DIR, OPAMP_AGENT_VERSION_ATTRIBUTE_KEY,
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
const AGENT_TYPE_NAMESPACE: &str = "onhost_install_agent_type";
const AGENT_TYPE_VERSION: &str = "0.1.0";

#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix)"]
fn test_local_agent_type_shadows_remote_registry_with_oci_registry() {
    let signer = OCISigner::start(tokio_runtime().handle().clone());

    let dirs = TempBasePaths::default();
    write_agent_type_to_local_dir(&dirs.local_dir());

    let mut opamp_server = FakeServer::start(tokio_runtime().handle());
    let mut agent_control =
        start_agent_control_for_test(&opamp_server, &signer, &dirs.base_paths());

    assert_sub_reports_version(&mut opamp_server, &dirs.base_paths());
    assert_agent_control_still_running(&mut agent_control);
}

#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix)"]
fn test_local_miss_resolves_via_remote_registry_with_oci_registry() {
    let signer = OCISigner::start(tokio_runtime().handle().clone());

    push_agent_type_to_registry(&signer);

    let mut opamp_server = FakeServer::start(tokio_runtime().handle());

    let dirs = TempBasePaths::default();

    let mut agent_control =
        start_agent_control_for_test(&opamp_server, &signer, &dirs.base_paths());

    assert_sub_reports_version(&mut opamp_server, &dirs.base_paths());

    assert_agent_control_still_running(&mut agent_control);
}

fn start_agent_control_for_test(
    opamp_server: &FakeServer,
    signer: &OCISigner,
    base_paths: &BasePaths,
) -> StartedAgentControl {
    OnHostAgentControlConfigBuilder::new(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_oci_registry(OCI_TEST_REGISTRY_URL)
        .with_agent_types(true, "test", signer.jwks_url().to_string())
        .write(base_paths.local_dir.clone());

    start_agent_control_with_custom_config(base_paths.clone(), AGENT_CONTROL_MODE_ON_HOST)
}

fn push_agent_type_to_registry(signer: &OCISigner) -> oci_client::Reference {
    let tag = AgentTypeTag::new(
        &agent_type_id(AGENT_TYPE_VERSION),
        AGENT_CONTROL_MODE_ON_HOST,
    );

    let source_dir = tempdir().unwrap();
    let archive_dir = tempdir().unwrap();

    let archive = create_agent_type_archive(&source_dir, &archive_dir, &tag);

    let reference = PackagePublisher::new(tokio_runtime().handle().clone(), OCI_TEST_REGISTRY_URL)
        .push_with_tag(&archive, AgentTypeArtifact, tag.as_str());

    signer.sign_artifact(&reference);

    push_test_package(
        signer,
        AGENT_TYPE_VERSION,
        OCI_TEST_REGISTRY_URL,
        "dummy.txt",
        "dummy package content",
    );

    reference
}

fn create_agent_type_archive(
    source_dir: &tempfile::TempDir,
    archive_dir: &tempfile::TempDir,
    tag: &AgentTypeTag,
) -> std::path::PathBuf {
    // Agent types are always tar.gz on all platforms, because they are platform-agnostic
    // (only contain YAML) and Agent Control always extracts them as tar.gz.
    let archive = archive_dir.path().join("agent-type.tar.gz");
    TestDataHelper::compress_tar_gz(
        source_dir.path(),
        &archive,
        &agent_type_definition_yaml(),
        &format!("{tag}.yaml"),
    );
    archive
}

fn write_agent_type_to_local_dir(local_dir: &Path) {
    let dynamic_dir = local_dir.join(DYNAMIC_AGENT_TYPES_DIR);
    std::fs::create_dir_all(&dynamic_dir).unwrap();
    std::fs::write(dynamic_dir.join("type.yaml"), agent_type_definition_yaml()).unwrap();
}

fn assert_sub_reports_version(opamp_server: &mut FakeServer, base_paths: &BasePaths) {
    let ac_instance_id = get_instance_id(&AgentID::AgentControl, base_paths.clone());

    let agent_type_id = agent_type_id(AGENT_TYPE_VERSION);

    let config = format!(
        r#"
agents:
  {AGENT_ID}:
    agent_type: "{agent_type_id}"
"#
    );

    opamp_server.set_config_response(ac_instance_id.clone(), config);

    retry(60, Duration::from_secs(1), || {
        opamp_server
            .is_config_status_applied(ac_instance_id.clone())
            .map_err(|e| e.into())
    });

    let sub_agent_id = AgentID::try_from(AGENT_ID).unwrap();

    let sub_agent_instance_id = get_instance_id(&sub_agent_id, base_paths.clone());

    let sub_config = format!("version: '{AGENT_TYPE_VERSION}'");

    opamp_server.set_config_response(sub_agent_instance_id.clone(), sub_config);

    retry(60, Duration::from_secs(1), || {
        check_identifying_attributes_contains_expected(
            opamp_server,
            &sub_agent_instance_id,
            convert_to_vec_key_value(vec![(
                OPAMP_AGENT_VERSION_ATTRIBUTE_KEY,
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

fn agent_type_definition_yaml() -> String {
    #[cfg(target_family = "unix")]
    let (operating_system, package_type) = ("linux", "tar");

    #[cfg(target_family = "windows")]
    let (operating_system, package_type) = ("windows", "zip");

    format!(
        r#"
namespace: {AGENT_TYPE_NAMESPACE}
name: {AGENT_TYPE_NAME}
version: {AGENT_TYPE_VERSION}
protocol_version: "1.0"
platform: host
operating_system: {operating_system}
variables:
  version:
    description: "Agent version"
    type: string
    required: false
    default: "{AGENT_TYPE_VERSION}"
deployment:
  packages:
    test-package:
      type: {package_type}
      download:
        oci:
          repository: test
          version: ${{nr-var:version}}
"#
    )
}
