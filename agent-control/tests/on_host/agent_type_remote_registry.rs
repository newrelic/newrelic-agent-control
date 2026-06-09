use crate::common::runtime::tokio_runtime;
use crate::on_host::tools::oci_package_manager::TestDataHelper;
use assert_matches::assert_matches;
use newrelic_agent_control::agent_control::config::Registry;
use newrelic_agent_control::agent_control::run::on_host::OCI_TEST_REGISTRY_URL;
use newrelic_agent_control::agent_type::agent_type_id::AgentTypeID;
use newrelic_agent_control::agent_type::oci::downloader::OCIAgentTypeArtifactDownloader;
use newrelic_agent_control::agent_type::registry::remote::RemoteRegistry;
use newrelic_agent_control::agent_type::registry::{AgentTypeRegistry, AgentTypeRegistryError};
use newrelic_agent_control::agent_type::runtime_config::on_host::package::rendered::Repository;
use newrelic_agent_control::environment::Environment;
use newrelic_agent_control::http::config::ProxyConfig;
use newrelic_agent_control::oci;
use oci_client::Reference;
use oci_client::client::{ClientConfig, ClientProtocol};
use oci_test_utils::{AgentTypeArtifact, OCISigner, PackagePublisher};
use std::str::FromStr;
use tempfile::tempdir;
use url::Url;

/// A minimal but valid kubernetes agent type definition.
fn agent_type_definition_yaml(name: &str, version: &str) -> String {
    format!(
        r#"
namespace: example
name: {name}
version: {version}
platform: kubernetes
deployment:
  objects: {{}}
"#
    )
}

/// Publishes an agent type artifact (a gzipped tar with the single definition file) to the test
/// registry under `tag`, optionally signing it, and returns the pushed reference.
fn push_agent_type(signer: Option<&OCISigner>, name: &str, version: &str, tag: &str) -> Reference {
    let source_dir = tempdir().unwrap();
    let archive_dir = tempdir().unwrap();
    let archive = archive_dir.path().join("agent-type.tar.gz");
    TestDataHelper::compress_tar_gz(
        source_dir.path(),
        &archive,
        &agent_type_definition_yaml(name, version),
        &format!("{tag}.yaml"),
    );

    let reference = PackagePublisher::new(tokio_runtime().handle().clone(), OCI_TEST_REGISTRY_URL)
        .push_with_tag(&archive, AgentTypeArtifact, tag);

    if let Some(signer) = signer {
        signer.sign_artifact(&reference);
    }
    reference
}

/// Builds a [RemoteRegistry] backed by a real downloader pointed at the test registry for the
/// kubernetes environment.
fn remote_registry(
    reference: &Reference,
    public_key_url: Url,
) -> RemoteRegistry<OCIAgentTypeArtifactDownloader> {
    let client = oci::Client::try_new(
        ClientConfig {
            protocol: ClientProtocol::Http,
            ..Default::default()
        },
        ProxyConfig::default(),
        tokio_runtime(),
    )
    .unwrap();

    let downloader = OCIAgentTypeArtifactDownloader::new(
        client,
        Registry::from_str(OCI_TEST_REGISTRY_URL).unwrap(),
        Repository::from_str(reference.repository()).unwrap(),
        None,
        Some(public_key_url),
    );

    RemoteRegistry::new(Environment::K8s, downloader)
}

#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix)"]
fn test_remote_registry_resolves_signed_agent_type_with_oci_registry() {
    let signer = OCISigner::start(tokio_runtime().handle().clone());
    let id = AgentTypeID::try_from("example/some.agent.type:0.0.123").unwrap();
    let reference = push_agent_type(
        Some(&signer),
        "some.agent.type",
        "0.0.123",
        "kubernetes-some.agent.type-0.0.123",
    );

    let registry = remote_registry(
        &reference,
        Url::parse(&signer.jwks_url().to_string()).unwrap(),
    );

    let definition = registry.get(&id).expect("signed agent type should resolve");
    assert_eq!(definition.metadata.id, id);
    assert_eq!(definition.metadata.environment, Environment::K8s);
}

#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix)"]
fn test_remote_registry_rejects_unsigned_agent_type_when_verification_enabled_with_oci_registry() {
    let signer = OCISigner::start(tokio_runtime().handle().clone());
    let id = AgentTypeID::try_from("example/some.agent.type:0.0.124").unwrap();
    // Pushed without signing while verification is enabled below.
    let reference = push_agent_type(
        None,
        "some.agent.type",
        "0.0.124",
        "kubernetes-some.agent.type-0.0.124",
    );

    let registry = remote_registry(
        &reference,
        Url::parse(&signer.jwks_url().to_string()).unwrap(),
    );

    assert_matches!(registry.get(&id), Err(AgentTypeRegistryError::Remote(_)));
}

#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix)"]
fn test_remote_registry_errors_on_missing_agent_type_with_oci_registry() {
    let signer = OCISigner::start(tokio_runtime().handle().clone());
    // Publish one agent type so the repository exists, then request a different, absent one.
    let reference = push_agent_type(
        Some(&signer),
        "some.agent.type",
        "0.0.125",
        "kubernetes-some.agent.type-0.0.125",
    );

    let registry = remote_registry(
        &reference,
        Url::parse(&signer.jwks_url().to_string()).unwrap(),
    );

    let missing = AgentTypeID::try_from("example/another.agent.type:9.9.9").unwrap();
    assert_matches!(
        registry.get(&missing),
        Err(AgentTypeRegistryError::Remote(_))
    );
}
