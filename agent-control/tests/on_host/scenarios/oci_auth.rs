use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::oci_signer::OCISigner;
use crate::common::opamp::FakeServer;
use crate::common::retry::retry;
use crate::on_host::tools::config::create_local_config;
use crate::on_host::tools::custom_agent_type::CustomAgentType;
use crate::on_host::tools::instance_id::get_instance_id;
use crate::on_host::tools::oci_artifact::push_agent_package_with_basic_auth;
use crate::on_host::tools::oci_artifact::{
    OCI_TEST_REGISTRY_BASIC_AUTH_PASSWORD, OCI_TEST_REGISTRY_BASIC_AUTH_USERNAME,
};
use crate::on_host::tools::oci_package_manager::TestDataHelper;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::defaults::AGENT_CONTROL_ID;
use newrelic_agent_control::agent_control::run::BasePaths;
use newrelic_agent_control::agent_control::run::on_host::{
    AGENT_CONTROL_MODE_ON_HOST, OCI_TEST_REGISTRY_URL,
};
use newrelic_agent_control::agent_control::version_updater::on_host::AGENT_CONTROL_BIN_PACKAGE_ID;
use newrelic_agent_control::package::oci::artifact_definitions::PackageMediaType;
use newrelic_agent_control::package::oci::package_manager::get_package_path;
use oci_client::Reference;
use oci_client::secrets::RegistryAuth;
use std::error::Error;
use std::path::Path;
use std::time::Duration;
use tempfile::tempdir;

const AGENT_ID: &str = "fake-agent";
const AGENT_PACKAGE_ID: &str = "test-package-id";

#[test]
#[ignore = "needs oci registry with basic auth (use *with_auth_oci_registry suffix)"]
fn test_agent_remote_package_with_auth_oci_registry() {
    let local_dir = tempdir().unwrap();
    let remote_dir = tempdir().unwrap();

    let signer = OCISigner::start();
    let opamp_server = FakeServer::start_new();

    let agent_type = CustomAgentType::default()
        .with_packages(Some(
            format!(
                r#"
{AGENT_PACKAGE_ID}:
  download:
    oci:
      repository: test
      version: ${{nr-var:fake_variable}}
      public_key_url: {}
"#,
                signer.jwks_url()
            )
            .as_str(),
        ))
        .build(local_dir.path().to_path_buf());

    let package_tag = push_fake_package_with_basic_auth(&signer);

    let agent_id = AgentID::try_from(AGENT_ID).unwrap();

    create_local_config(
        agent_id.as_str(),
        format!("fake_variable: '{package_tag}'").to_string(),
        local_dir.path().to_path_buf(),
    );
    create_ac_local_config(
        &opamp_server,
        &signer,
        local_dir.path(),
        format!(r#"{{ "{AGENT_ID}": {{ "agent_type": "{agent_type}" }} }}"#),
    );

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };
    let _agent_control =
        start_agent_control_with_custom_config(base_paths.clone(), AGENT_CONTROL_MODE_ON_HOST);

    retry(60, Duration::from_secs(1), || {
        verify_fake_agent_has_been_pulled(remote_dir.path(), &package_tag)
    });
}

#[test]
#[ignore = "needs oci registry with basic auth (use *with_auth_oci_registry suffix)"]
fn test_ac_self_update_with_auth_oci_registry() {
    let local_dir = tempdir().unwrap();
    let remote_dir = tempdir().unwrap();

    let signer = OCISigner::start();
    let mut opamp_server = FakeServer::start_new();

    let package_tag = push_fake_package_with_basic_auth(&signer);

    create_ac_local_config(&opamp_server, &signer, local_dir.path(), "{}");

    let base_paths = BasePaths {
        local_dir: local_dir.path().to_path_buf(),
        remote_dir: remote_dir.path().to_path_buf(),
        log_dir: local_dir.path().to_path_buf(),
    };
    let _agent_control =
        start_agent_control_with_custom_config(base_paths.clone(), AGENT_CONTROL_MODE_ON_HOST);

    let ac_instance_id = get_instance_id(&AgentID::AgentControl, base_paths.clone());

    let update_config = format!(
        r#"
version: "{}"
agents: {{}}
"#,
        package_tag
    );
    opamp_server.set_config_response(ac_instance_id.clone(), update_config);

    // We just verify the package has been pulled, other scenarios are covered in ac_self_update.rs
    retry(60, Duration::from_secs(1), || {
        verify_fake_ac_has_been_pulled(remote_dir.path(), &package_tag)
    });
}

const FAKE_ARTIFACT_NAME: &str = "fake-artifact-name";

fn push_fake_package_with_basic_auth(signer: &OCISigner) -> String {
    let dir = tempdir().unwrap();
    let tmp_dir_to_compress = tempdir().unwrap();

    let path = dir.path().join("layer_digest.tar.gz");
    TestDataHelper::compress_tar_gz(
        tmp_dir_to_compress.path(),
        &path,
        "fake content",
        FAKE_ARTIFACT_NAME,
    );
    let (_, reference) = push_agent_package_with_basic_auth(
        &path,
        OCI_TEST_REGISTRY_URL,
        PackageMediaType::AgentPackageLayerTarGz,
    );

    signer.sign_artifact_with_auth(
        &reference,
        RegistryAuth::Basic(
            OCI_TEST_REGISTRY_BASIC_AUTH_USERNAME.to_string(),
            OCI_TEST_REGISTRY_BASIC_AUTH_PASSWORD.to_string(),
        ),
    );

    reference.tag().unwrap().to_string()
}

fn verify_fake_agent_has_been_pulled(
    remote_dir: &Path,
    tag: impl Into<String>,
) -> Result<(), Box<dyn Error>> {
    verify_fake_artifact_has_been_pulled(
        remote_dir,
        &AgentID::try_from(AGENT_ID).unwrap(),
        AGENT_PACKAGE_ID,
        tag,
    )
}
fn verify_fake_ac_has_been_pulled(
    remote_dir: &Path,
    tag: impl Into<String>,
) -> Result<(), Box<dyn Error>> {
    verify_fake_artifact_has_been_pulled(
        remote_dir,
        &AgentID::AgentControl,
        AGENT_CONTROL_BIN_PACKAGE_ID,
        tag,
    )
}
fn verify_fake_artifact_has_been_pulled(
    remote_dir: &Path,
    agent_id: &AgentID,
    package_id: impl Into<String>,
    tag: impl Into<String>,
) -> Result<(), Box<dyn Error>> {
    // TODO bug workaround , remove when rebasing to master
    let ref_fix = Reference::with_tag("base.io".to_string(), "test".to_string(), tag.into());
    let package_path =
        get_package_path(remote_dir, agent_id, &package_id.into(), &ref_fix).unwrap();

    dbg!("### Expected Package path: {:?}", &package_path);

    if package_path.join(FAKE_ARTIFACT_NAME).exists() {
        Ok(())
    } else {
        Err("package not pulled yet".into())
    }
}

fn create_ac_local_config(
    opamp_server: &FakeServer,
    signer: &OCISigner,
    local_dir: &Path,
    agents: impl Into<String>,
) {
    let config = format!(
        r#"
agents: {}
host_id: integration-test
fleet_control:
  endpoint: {}
  poll_interval: 5s
  signature_validation:
    public_key_server_url: {}
oci:
  registry: "{OCI_TEST_REGISTRY_URL}"
  auth:
    basic:
      username: {OCI_TEST_REGISTRY_BASIC_AUTH_USERNAME}
      password: {OCI_TEST_REGISTRY_BASIC_AUTH_PASSWORD}
self_update:
  enabled: true
  signature_verification_enabled: true
  package:
    download:
      oci:
        # TODO remove this when rebased to master!!!!!
        registry: {OCI_TEST_REGISTRY_URL}
        repository: test
        public_key_url: {}
"#,
        agents.into(),
        opamp_server.endpoint(),
        opamp_server.jwks_endpoint(),
        signer.jwks_url()
    );
    create_local_config(AGENT_CONTROL_ID, config, local_dir.to_path_buf());
}
