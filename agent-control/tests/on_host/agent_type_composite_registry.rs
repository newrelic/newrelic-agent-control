use crate::common::runtime::tokio_runtime;
use crate::on_host::tools::oci_package_manager::TestDataHelper;
use newrelic_agent_control::agent_control::config::Registry as OciRegistryConfig;
use newrelic_agent_control::agent_control::run::on_host::OCI_TEST_REGISTRY_URL;
use newrelic_agent_control::agent_type::agent_type_id::AgentTypeID;
use newrelic_agent_control::agent_type::oci::downloader::OCIAgentTypeArtifactDownloader;
use newrelic_agent_control::agent_type::registry::{AgentTypeRegistry, Registry, RegistryConfig};
use newrelic_agent_control::agent_type::runtime_config::on_host::package::rendered::Repository;
use newrelic_agent_control::environment::Environment;
use newrelic_agent_control::http::config::ProxyConfig;
use newrelic_agent_control::oci;
use oci_client::Reference;
use oci_client::client::{ClientConfig, ClientProtocol};
use oci_test_utils::{AgentTypeArtifact, OCISigner, PackagePublisher};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::str::FromStr;
use tempfile::{TempDir, tempdir};
use url::Url;

/// A minimal but valid kubernetes agent type definition. The marker variable is what each test
/// uses to assert which layer of the composite served the request.
fn agent_type_definition_yaml(name: &str, version: &str, marker: &str) -> String {
    format!(
        r#"
namespace: composite-registry
name: {name}
version: {version}
protocol_version: "1.0"
platform: kubernetes
variables:
  {marker}:
    type: string
    required: true
    description: "marker"
deployment:
  objects: {{}}
"#
    )
}

/// Publishes an agent type artifact (a gzipped tar with the single definition file) to the test
/// registry under `tag`, optionally signing it. Returns the pushed reference.
fn push_agent_type(
    signer: Option<&OCISigner>,
    name: &str,
    version: &str,
    tag: &str,
    marker: &str,
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

/// Writes the given YAML to `<dir>/<filename>`. The local registry loads everything in
/// `dynamic_agent_types_path` whose env matches the running one.
fn write_yaml(dir: &Path, filename: &str, content: &str) {
    File::create(dir.join(filename))
        .unwrap()
        .write_all(content.as_bytes())
        .unwrap();
}

/// Builds the production composite [Registry] (local + remote) for the kubernetes environment,
/// pointed at the test OCI registry.
fn composite_registry(
    dynamic_dir: &TempDir,
    repository: &str,
    public_key_url: Option<Url>,
) -> Registry {
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
        OciRegistryConfig::from_str(OCI_TEST_REGISTRY_URL).unwrap(),
        Repository::from_str(repository).unwrap(),
        None,
        public_key_url,
    );

    Registry::build(
        Environment::K8s,
        RegistryConfig {
            dynamic_agent_types_path: dynamic_dir.path().to_path_buf(),
        },
        downloader,
    )
}

#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix)"]
fn test_local_layer_shadows_remote_with_oci_registry() {
    let signer = OCISigner::start(tokio_runtime().handle().clone());
    let id = AgentTypeID::try_from("composite-registry/some.agent.type:0.0.1").unwrap();

    // Add agent type to remote registry with a `remote_marker`.
    let reference = push_agent_type(
        Some(&signer),
        "some.agent.type",
        "0.0.1",
        "kubernetes-some.agent.type-0.0.1",
        "remote_marker",
    );

    // Add agent type for the same id in the local dir with a different marker.
    let dynamic_dir = tempdir().unwrap();
    write_yaml(
        dynamic_dir.path(),
        "local.yaml",
        &agent_type_definition_yaml("some.agent.type", "0.0.1", "local_marker"),
    );

    let registry = composite_registry(
        &dynamic_dir,
        reference.repository(),
        Some(Url::parse(&signer.jwks_url().to_string()).unwrap()),
    );

    let definition = registry.get(&id).expect("local should resolve the id");
    assert_eq!(definition.metadata.id, id);
    assert_eq!(definition.metadata.environment, Environment::K8s);

    let variables = definition.variables.clone().flatten();
    assert!(variables.contains_key("local_marker"));
    assert!(!variables.contains_key("remote_marker"));
}

#[test]
#[ignore = "needs oci registry (use *with_oci_registry suffix)"]
fn test_local_miss_resolves_via_remote_with_oci_registry() {
    let signer = OCISigner::start(tokio_runtime().handle().clone());
    let id = AgentTypeID::try_from("composite-registry/some.agent.type:0.0.2").unwrap();

    let reference = push_agent_type(
        Some(&signer),
        "some.agent.type",
        "0.0.2",
        "kubernetes-some.agent.type-0.0.2",
        "remote_marker",
    );

    let dynamic_dir = tempdir().unwrap();
    let registry = composite_registry(
        &dynamic_dir,
        reference.repository(),
        Some(Url::parse(&signer.jwks_url().to_string()).unwrap()),
    );

    let definition = registry.get(&id).expect("remote should resolve the id");
    assert_eq!(definition.metadata.id, id);
    assert_eq!(definition.metadata.environment, Environment::K8s);
}
