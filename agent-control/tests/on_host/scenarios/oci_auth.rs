use crate::common::agent_control::start_agent_control_with_custom_config;
use crate::common::base_paths::TempBasePaths;
use crate::common::retry::retry;
use crate::common::runtime::tokio_runtime;
use crate::on_host::tools::config::{AgentControlConfigBuilder, create_local_config};
use crate::on_host::tools::custom_agent_type::CustomAgentType;
use crate::on_host::tools::instance_id::get_instance_id;
use crate::on_host::tools::oci_package_manager::TestDataHelper;
use fake_opamp_server::FakeServer;
use newrelic_agent_control::agent_control::agent_id::AgentID;
use newrelic_agent_control::agent_control::run::on_host::{
    AGENT_CONTROL_MODE_ON_HOST, OCI_TEST_REGISTRY_URL,
};
use newrelic_agent_control::agent_control::version_updater::on_host::AGENT_CONTROL_BIN_PACKAGE_ID;
use newrelic_agent_control::agent_type::runtime_config::on_host::package::rendered::Oci;
use newrelic_agent_control::package::manager::PackageData;
use newrelic_agent_control::package::oci::package_manager::get_package_path;
use oci_client::Reference;
use oci_client::secrets::RegistryAuth;
use oci_test_utils::{OCISigner, PackagePublisher};
use std::error::Error;
use std::path::Path;
use std::time::Duration;
use tempfile::tempdir;

const AGENT_ID: &str = "fake-agent";
const AGENT_PACKAGE_ID: &str = "test-package-id";
const OCI_TEST_REGISTRY_BASIC_AUTH_USERNAME: &str = "fake-user";
const OCI_TEST_REGISTRY_BASIC_AUTH_PASSWORD: &str = "fake-password";

#[test]
#[ignore = "needs oci registry with basic auth (use *with_auth_oci_registry suffix)"]
fn test_agent_remote_package_with_auth_oci_registry() {
    let opamp_server = FakeServer::start(tokio_runtime().handle());
    let signer = OCISigner::start(tokio_runtime().handle().clone());

    let dirs = TempBasePaths::default();

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
        .build(dirs.local_dir());

    let package_reference = push_fake_package_with_basic_auth(&signer);
    let package_tag = package_reference.tag().unwrap().to_string();

    let agent_id = AgentID::try_from(AGENT_ID).unwrap();

    create_local_config(
        agent_id.as_str(),
        format!("fake_variable: '{package_tag}'").to_string(),
        dirs.local_dir(),
    );
    create_ac_local_config(
        &opamp_server,
        &signer,
        &dirs.local_dir(),
        format!(r#"{{ "{AGENT_ID}": {{ "agent_type": "{agent_type}" }} }}"#),
    );

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    retry(60, Duration::from_secs(1), || {
        verify_fake_agent_has_been_pulled(&dirs.remote_dir(), &package_reference)
    });
}

#[test]
#[ignore = "needs oci registry with basic auth (use *with_auth_oci_registry suffix)"]
fn test_ac_self_update_with_auth_oci_registry() {
    let dirs = TempBasePaths::default();

    let mut opamp_server = FakeServer::start(tokio_runtime().handle());
    let signer = OCISigner::start(tokio_runtime().handle().clone());

    let package_reference = push_fake_package_with_basic_auth(&signer);
    let package_tag = package_reference.tag().unwrap().to_string();

    create_ac_local_config(&opamp_server, &signer, &dirs.local_dir(), "{}");

    let _agent_control =
        start_agent_control_with_custom_config(dirs.base_paths(), AGENT_CONTROL_MODE_ON_HOST);

    let ac_instance_id = get_instance_id(&AgentID::AgentControl, dirs.base_paths());

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
        verify_fake_ac_has_been_pulled(&dirs.remote_dir(), &package_reference)
    });
}

const FAKE_ARTIFACT_NAME: &str = "fake-artifact-name";

fn push_fake_package_with_basic_auth(signer: &OCISigner) -> Reference {
    let dir = tempdir().unwrap();
    let tmp_dir_to_compress = tempdir().unwrap();

    let path = dir.path().join("layer_digest.tar.gz");
    TestDataHelper::compress_tar_gz(
        tmp_dir_to_compress.path(),
        &path,
        // force different digests for each test to avoid race conditions
        // as the OCISigner wipes out old signatures.
        format!("fake random data: {}", &dir.path().display()).as_str(),
        FAKE_ARTIFACT_NAME,
    );
    let reference = PackagePublisher::new(tokio_runtime().handle().clone(), OCI_TEST_REGISTRY_URL)
        .with_basic_auth(
            OCI_TEST_REGISTRY_BASIC_AUTH_USERNAME,
            OCI_TEST_REGISTRY_BASIC_AUTH_PASSWORD,
        )
        .push(&path, oci_test_utils::PackageMediaType::TarGz);

    signer.sign_artifact_with_auth(
        &reference,
        RegistryAuth::Basic(
            OCI_TEST_REGISTRY_BASIC_AUTH_USERNAME.to_string(),
            OCI_TEST_REGISTRY_BASIC_AUTH_PASSWORD.to_string(),
        ),
    );

    reference
}

fn verify_fake_agent_has_been_pulled(
    remote_dir: &Path,
    reference: &Reference,
) -> Result<(), Box<dyn Error>> {
    verify_fake_artifact_has_been_pulled(
        remote_dir,
        &AgentID::try_from(AGENT_ID).unwrap(),
        &PackageData {
            id: AGENT_PACKAGE_ID.to_string(),
            oci: Oci {
                repository: reference.repository().parse().unwrap(),
                version: reference.tag().unwrap().parse().unwrap(),
                public_key_url: None,
            },
        },
    )
}
fn verify_fake_ac_has_been_pulled(
    remote_dir: &Path,
    reference: &Reference,
) -> Result<(), Box<dyn Error>> {
    verify_fake_artifact_has_been_pulled(
        remote_dir,
        &AgentID::AgentControl,
        &PackageData {
            id: AGENT_CONTROL_BIN_PACKAGE_ID.to_string(),
            oci: Oci {
                repository: reference.repository().parse().unwrap(),
                version: reference.tag().unwrap().parse().unwrap(),
                public_key_url: None,
            },
        },
    )
}
fn verify_fake_artifact_has_been_pulled(
    remote_dir: &Path,
    agent_id: &AgentID,
    package_data: &PackageData,
) -> Result<(), Box<dyn Error>> {
    let package_path = get_package_path(remote_dir, agent_id, package_data).unwrap();

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
    AgentControlConfigBuilder::basic(opamp_server.endpoint(), opamp_server.jwks_endpoint())
        .with_agents(agents)
        .with_oci_registry(OCI_TEST_REGISTRY_URL)
        .with_oci_basic_auth(
            OCI_TEST_REGISTRY_BASIC_AUTH_USERNAME,
            OCI_TEST_REGISTRY_BASIC_AUTH_PASSWORD,
        )
        .with_self_update(true, "test", signer.jwks_url().to_string())
        .write(local_dir.to_path_buf());
}
