//! Pieces of the OHI agent scenario that do not depend on the operating system.

use crate::common::InstallationArgs;
use crate::common::agent_metadata::{AgentKind, AgentMetadata, AgentMetadataSet};
use crate::common::docker_hub::{AGENT_TYPES_REPOSITORY, INFRA_AGENT_REPOSITORY};
use crate::common::nrql::Region;
use crate::common::oci::OciRegistry;
use crate::common::runtime::tokio_runtime;
use chrono::{Local, Timelike};
use oci_client::Reference;
use oci_test_utils::{AgentTypeDefinitionMeta, PackageMediaType, PackagePublisher};
use serde_json::{Value, json};
use std::fs::read_to_string;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const TEST_ID_PLACEHOLDER: &str = "{{test_id}}";
pub const PACKAGE_REPOSITORY: &str = "ohi-agent";
pub const AGENT_TYPE_REPOSITORY: &str = "ohi-agent-types";
pub const NRQL_ASSERTION_RETRIES: i64 = 60;
pub const POLL_INTERVAL: Duration = Duration::from_secs(10);

/// Arguments of the scenario that tests an OHI
#[derive(Debug, Clone, clap::Parser)]
pub struct OhiAgentArgs {
    /// Agent type definition for this operating system
    #[arg(long)]
    pub agent_type_file: PathBuf,

    /// Final package archive (.tar.gz, .tgz or .zip) with the agent binary at its root
    #[arg(long)]
    pub package_file: PathBuf,

    /// Local config of the sub-agent (the values the agent type declares, e.g. `config:`)
    #[arg(long)]
    pub config_file: PathBuf,

    /// NRQL that must return a meaningful result. `{{test_id}}` is replaced with the id of this test
    #[arg(long)]
    pub nrql_assertion: String,

    /// New Relic license key for the infrastructure agent
    #[arg(long)]
    pub nr_license_key: String,

    /// New Relic API key used to run the NRQL assertion
    #[arg(long)]
    pub nr_api_key: String,

    /// New Relic account identifier
    #[arg(long)]
    pub nr_account_id: String,

    /// New Relic region
    #[arg(long, ignore_case = true)]
    pub nr_region: Region,

    /// Agent versions resolved by the `agent-versions` action (its `json` output)
    #[arg(long, env = "AGENT_METADATA")]
    pub agent_metadata: AgentMetadataSet,
}

impl OhiAgentArgs {
    pub fn agent(&self, agent: AgentKind) -> AgentMetadata {
        self.agent_metadata.get(agent).clone()
    }

    pub fn installation_args(&self) -> InstallationArgs {
        InstallationArgs {
            nr_api_key: self.nr_api_key.clone(),
            nr_license_key: self.nr_license_key.clone(),
            nr_account_id: self.nr_account_id.clone(),
            nr_region: self.nr_region,
            ..Default::default()
        }
    }
}

/// Identifies one execution: `H.M.(seconds*1000+millis)`. Valid semver because no part has leading zeros.
pub fn test_id() -> String {
    let now = Local::now();
    format!(
        "{}.{}.{}",
        now.hour(),
        now.minute(),
        now.second() * 1000 + now.timestamp_subsec_millis()
    )
}

/// Adds the values that make the agent type pull the package of this test from the test registry.
pub fn render_agent_local_config(input_config: &str, test_id: &str) -> String {
    let mut config: Value = serde_saphyr::from_str(input_config)
        .unwrap_or_else(|err| panic!("invalid config file YAML: {err}"));
    if config.is_null() {
        config = json!({});
    }
    let mapping = config
        .as_object_mut()
        .unwrap_or_else(|| panic!("the config file must be a YAML mapping"));

    for reserved in ["version", "oci"] {
        assert!(
            !mapping.contains_key(reserved),
            "the config file already sets '{reserved}', which the scenario injects"
        );
    }
    mapping.insert("version".to_string(), json!(test_id));
    mapping.insert(
        "oci".to_string(),
        json!({ "repository": PACKAGE_REPOSITORY }),
    );
    serde_saphyr::to_string(&config)
        .unwrap_or_else(|err| panic!("could not serialize the agent config: {err}"))
}

pub fn substitute_test_id(nrql: &str, test_id: &str) -> String {
    assert!(
        nrql.contains(TEST_ID_PLACEHOLDER),
        "the NRQL assertion must contain {TEST_ID_PLACEHOLDER} to only match data of this test"
    );
    nrql.replace(TEST_ID_PLACEHOLDER, test_id)
}

pub fn package_media_type(path: &Path) -> PackageMediaType {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if file_name.ends_with(".tar.gz") || file_name.ends_with(".tgz") {
        PackageMediaType::TarGz
    } else if file_name.ends_with(".zip") {
        PackageMediaType::Zip
    } else {
        panic!("unsupported package '{file_name}', expected .tar.gz, .tgz or .zip")
    }
}

fn publisher(registry: &OciRegistry, repository: &str) -> PackagePublisher {
    PackagePublisher::new(tokio_runtime().handle().clone(), registry.url())
        .with_repository(repository)
}

pub fn push_agent_package(
    registry: &OciRegistry,
    package: &Path,
    media_type: PackageMediaType,
    test_id: &str,
) -> Reference {
    publisher(registry, PACKAGE_REPOSITORY).push_with_tag(package, media_type, test_id)
}

/// Replaces the first line starting with `version:` at column 0, keeping its line ending.
fn with_top_level_version(yaml: &str, version: &str) -> String {
    let mut replaced = false;
    let rewritten: String = yaml
        .split_inclusive('\n')
        .map(|line| {
            if !replaced && line.starts_with("version:") {
                replaced = true;
                let ending = &line[line.trim_end_matches(['\r', '\n']).len()..];
                format!("version: {version}{ending}")
            } else {
                line.to_string()
            }
        })
        .collect();
    assert!(replaced, "the agent type has no top-level 'version:' line");
    rewritten
}

/// Pushes the agent type under a version unique to this test. Embedded definitions take precedence
/// over remote ones, so keeping the original version could resolve to a built-in with the same id
/// instead of the pushed definition.
pub fn push_agent_type(
    registry: &OciRegistry,
    definition: &Path,
    test_id: &str,
) -> (Reference, AgentTypeDefinitionMeta) {
    let yaml = read_to_string(definition)
        .unwrap_or_else(|err| panic!("could not read '{}': {err}", definition.display()));
    let yaml = with_top_level_version(&yaml, test_id);
    publisher(registry, AGENT_TYPE_REPOSITORY)
        .push_agent_type(&yaml)
        .unwrap_or_else(|err| panic!("could not push the agent type: {err}"))
}

/// Copies the infra agent package to the test registry. The infra agent hosts the OHI under test and
/// AC pulls every package from `oci.registry`, so the package must be served from the test registry.
pub fn mirror_infra_agent(registry: &OciRegistry, version: &str) -> Reference {
    let source: Reference = format!("docker.io/{INFRA_AGENT_REPOSITORY}:{version}")
        .parse()
        .unwrap_or_else(|err| panic!("invalid infra agent reference: {err}"));
    publisher(registry, INFRA_AGENT_REPOSITORY)
        .mirror(&source, version)
        .unwrap_or_else(|err| panic!("could not mirror the infra agent package: {err}"))
}

/// Copies the published agent type `agent_type_id` (`<namespace>/<name>:<version>`) to the test
/// registry, where AC looks up every remote agent type. `environment_prefix` is the platform part
/// of the tag, e.g. `host-linux`.
pub fn mirror_agent_type(
    registry: &OciRegistry,
    agent_type_id: &str,
    environment_prefix: &str,
) -> Reference {
    let (qualified_name, version) = agent_type_id
        .split_once(':')
        .unwrap_or_else(|| panic!("invalid agent type id '{agent_type_id}'"));
    let name = qualified_name.rsplit('/').next().unwrap_or(qualified_name);
    let tag = format!("{environment_prefix}-{name}-{version}");
    let source: Reference = format!("docker.io/{AGENT_TYPES_REPOSITORY}:{tag}")
        .parse()
        .unwrap_or_else(|err| panic!("invalid agent type reference: {err}"));
    publisher(registry, AGENT_TYPE_REPOSITORY)
        .mirror(&source, &tag)
        .unwrap_or_else(|err| panic!("could not mirror the agent type '{agent_type_id}': {err}"))
}
