use crate::common::runtime::tokio_runtime;
use crate::on_host::tools::oci_package_manager::TestDataHelper;
use assert_matches::assert_matches;
use newrelic_agent_control::agent_control::config::{
    AgentTypeConfig, DefaultAgentTypeRemote, OciConfig, Registry,
};
use newrelic_agent_control::agent_control::run::build_agent_type_registry;
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
use std::path::Path;
use std::str::FromStr;
use tempfile::tempdir;
use url::Url;

/// A well-known agent type embedded in the binary
const EMBEDDED_AGENT_TYPE_ID: &str = "newrelic/com.newrelic.infrastructure:0.1.0";
/// Non-existent folder for local agent-types
const NONEXISTENT_DYNAMIC_AGENT_TYPES_DIR: &str = "/nonexistent-agent-type-registry-test-dir";

/// A minimal but valid kubernetes agent type definition. `marker` is an arbitrary variable default
/// with no effect on identity, used to distinguish two revisions pushed under the same
/// namespace/name/version (and therefore the same tag).
fn agent_type_definition_yaml(name: &str, version: &str, marker: &str) -> String {
    format!(
        r#"
namespace: example
name: {name}
version: {version}
protocol_version: "1.0"
platform: kubernetes
variables:
  marker:
    description: "distinguishes artifact revisions in tests"
    type: string
    required: false
    default: "{marker}"
deployment:
  objects: {{}}
"#
    )
}

/// Publishes an agent type artifact (a gzipped tar with the single definition file) to the test
/// registry under `tag`, optionally signing it, and returns the pushed reference.
fn push_agent_type(
    signer: Option<&OCISigner>,
    name: &str,
    version: &str,
    marker: &str,
    tag: &str,
) -> Reference {
    let source_dir = tempdir().unwrap();
    let archive_dir = tempdir().unwrap();
    let archive = archive_dir.path().join("agent-type.tar.gz");
    TestDataHelper::compress_tar_gz(
        source_dir.path(),
        &archive,
        &agent_type_definition_yaml(name, version, marker),
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

/// Builds an [`oci::Client`] pointed at the local, unencrypted test registry.
fn test_oci_client() -> oci::Client {
    oci::Client::try_new(
        ClientConfig {
            protocol: ClientProtocol::Http,
            ..Default::default()
        },
        ProxyConfig::default(),
        tokio_runtime(),
    )
    .unwrap()
}

/// Builds the `(AgentTypeConfig, OciConfig)` pair that [`build_agent_type_registry`] needs,
/// pointed at the test registry/repository holding `reference` and verifying signatures against
/// `public_key_url`.
fn agent_type_and_oci_config(
    reference: &Reference,
    public_key_url: Url,
) -> (AgentTypeConfig, OciConfig) {
    let agent_types = AgentTypeConfig {
        default_remote: DefaultAgentTypeRemote {
            repository: Repository::from_str(reference.repository()).unwrap(),
            public_key_url,
            ..Default::default()
        },
    };
    let oci = OciConfig {
        registry: Registry::from_str(OCI_TEST_REGISTRY_URL).unwrap(),
        auth: None,
    };
    (agent_types, oci)
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
        "n/a",
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
        "n/a",
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
        "n/a",
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

#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix)"]
fn test_build_agent_type_registry_resolves_embedded_and_caches_remote_with_oci_registry() {
    let signer = OCISigner::start(tokio_runtime().handle().clone());

    let embedded_id = AgentTypeID::try_from(EMBEDDED_AGENT_TYPE_ID).unwrap();
    let remote_id = AgentTypeID::try_from("example/some.agent.type:0.0.126").unwrap();
    let tag = "kubernetes-some.agent.type-0.0.126";

    let reference = push_agent_type(Some(&signer), "some.agent.type", "0.0.126", "first", tag);
    let public_key_url = Url::parse(&signer.jwks_url().to_string()).unwrap();
    let local_dir = Path::new(NONEXISTENT_DYNAMIC_AGENT_TYPES_DIR);

    let (agent_types, oci) = agent_type_and_oci_config(&reference, public_key_url.clone());
    let registry = build_agent_type_registry(
        &agent_types,
        &oci,
        Environment::K8s,
        local_dir,
        test_oci_client(),
    )
    .expect("registry should build");

    // Check embedded agent-type
    let definition = registry
        .get(&embedded_id)
        .expect("embedded agent type should resolve without an OCI registry");
    assert_eq!(definition.metadata.id, embedded_id);
    assert_eq!(definition.metadata.environment, Environment::K8s);

    // Not present locally, so this resolves via the remote layer and populates its cache.
    let remote_definition = registry
        .get(&remote_id)
        .expect("first lookup should resolve via the remote layer");
    assert_eq!(remote_definition.metadata.id, remote_id);

    // Overwrite the same tag with a different, signed revision (same identity, different content) to prove that
    // cache works.
    push_agent_type(Some(&signer), "some.agent.type", "0.0.126", "second", tag);

    // Check that the registry returns the cached definition
    let cached_definition = registry
        .get(&remote_id)
        .expect("second lookup should be served from the cache");
    assert_eq!(cached_definition, remote_definition);
}
