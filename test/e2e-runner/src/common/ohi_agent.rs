//! Pieces of the OHI agent scenario that do not depend on the operating system.

use crate::common::InstallationArgs;
use crate::common::docker_hub::INFRA_AGENT_REPOSITORY;
use crate::common::nrql::{Region, check_query_results};
use crate::common::oci::OciRegistry;
use crate::common::runtime::tokio_runtime;
use crate::common::test::TestResult;
use chrono::{Local, Timelike};
use oci_client::Reference;
use oci_test_utils::{
    AgentTypeDefinitionMeta, PackageMediaType, PackagePublisher, mirror_host_package,
};
use reqwest::blocking::Client;
use serde_json::{Value, json};
use std::fs::read_to_string;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::{info, warn};

pub const TEST_ID_PLACEHOLDER: &str = "{{test_id}}";
pub const PACKAGE_REPOSITORY: &str = "ohi-agent";
pub const AGENT_TYPE_REPOSITORY: &str = "ohi-agent-types";

pub const POLL_INTERVAL: Duration = Duration::from_secs(10);
const STATUS_URL: &str = "http://localhost:51200/status";
const STATUS_TIMEOUT: Duration = Duration::from_secs(10);

/// Arguments of the scenario that tests an agent provided by its owner.
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

    /// NRQL that must return a meaningful result. `{{test_id}}` is replaced with the id of this run
    #[arg(long)]
    pub nrql_assertion: String,

    /// New Relic license key for the infrastructure agent
    #[arg(long, env = "NR_LICENSE_KEY")]
    pub nr_license_key: String,

    /// New Relic API key used to run the NRQL assertion
    #[arg(long, env = "NR_API_KEY")]
    pub nr_api_key: String,

    /// New Relic account identifier
    #[arg(long, env = "NR_ACCOUNT_ID")]
    pub nr_account_id: String,

    /// New Relic region
    #[arg(long, env = "NR_REGION", ignore_case = true)]
    pub nr_region: Region,

    /// Seconds to wait for the assertion to pass
    #[arg(long, default_value_t = 600)]
    pub timeout_seconds: u64,

    /// Recipes repository
    #[arg(
        long,
        default_value = "https://github.com/newrelic/open-install-library.git"
    )]
    pub recipes_repo: String,

    /// Recipes repository branch
    #[arg(long, default_value = "main")]
    pub recipes_repo_branch: String,
}

impl OhiAgentArgs {
    pub fn installation_args(&self, agent_control_version: String) -> InstallationArgs {
        InstallationArgs {
            recipes_repo: self.recipes_repo.clone(),
            recipes_repo_branch: self.recipes_repo_branch.clone(),
            nr_api_key: self.nr_api_key.clone(),
            nr_license_key: self.nr_license_key.clone(),
            nr_account_id: self.nr_account_id.clone(),
            nr_region: self.nr_region,
            agent_control_version,
            ..Default::default()
        }
    }

    pub fn poll_retries(&self) -> i64 {
        (self.timeout_seconds / POLL_INTERVAL.as_secs()).max(1) as i64
    }
}

/// Identifies one execution: `H.M.(seconds*1000+millis)`. Valid semver because no part has leading zeros.
pub fn run_id() -> String {
    let now = Local::now();
    format!(
        "{}.{}.{}",
        now.hour(),
        now.minute(),
        now.second() * 1000 + now.timestamp_subsec_millis()
    )
}

/// Adds the values that make the agent type pull the package of this run from the test registry.
pub fn render_agent_local_config(owner_config: &str, run_id: &str) -> TestResult<String> {
    let mut config: Value = serde_saphyr::from_str(owner_config)
        .map_err(|err| format!("invalid config file YAML: {err}"))?;
    if config.is_null() {
        config = json!({});
    }
    let mapping = config
        .as_object_mut()
        .ok_or("the config file must be a YAML mapping")?;

    for reserved in ["version", "oci"] {
        if mapping.contains_key(reserved) {
            return Err(format!(
                "the config file already sets '{reserved}', which the scenario injects"
            )
            .into());
        }
    }
    mapping.insert("version".to_string(), json!(run_id));
    mapping.insert(
        "oci".to_string(),
        json!({ "repository": PACKAGE_REPOSITORY }),
    );

    Ok(serde_saphyr::to_string(&config)?)
}

pub fn substitute_test_id(nrql: &str, test_id: &str) -> TestResult<String> {
    if !nrql.contains(TEST_ID_PLACEHOLDER) {
        return Err(format!(
            "the NRQL assertion must contain {TEST_ID_PLACEHOLDER} to only match data of this run"
        )
        .into());
    }
    Ok(nrql.replace(TEST_ID_PLACEHOLDER, test_id))
}

pub fn package_media_type(path: &Path) -> TestResult<PackageMediaType> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if file_name.ends_with(".tar.gz") || file_name.ends_with(".tgz") {
        Ok(PackageMediaType::TarGz)
    } else if file_name.ends_with(".zip") {
        Ok(PackageMediaType::Zip)
    } else {
        Err(format!("unsupported package '{file_name}', expected .tar.gz, .tgz or .zip").into())
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
    run_id: &str,
) -> Reference {
    publisher(registry, PACKAGE_REPOSITORY).push_with_tag(package, media_type, run_id)
}

/// Replaces the first line starting with `version:` at column 0, keeping its line ending.
fn with_top_level_version(yaml: &str, version: &str) -> TestResult<String> {
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
    if !replaced {
        return Err("the agent type has no top-level 'version:' line".into());
    }
    Ok(rewritten)
}

pub fn push_agent_type(
    registry: &OciRegistry,
    definition: &Path,
    run_id: &str,
) -> TestResult<(Reference, AgentTypeDefinitionMeta)> {
    let yaml = read_to_string(definition)
        .map_err(|err| format!("could not read '{}': {err}", definition.display()))?;
    let yaml = with_top_level_version(&yaml, run_id)?;
    publisher(registry, AGENT_TYPE_REPOSITORY).push_agent_type(&yaml)
}

/// Copies the infra agent package to the test registry, since AC pulls every package from `oci.registry`.
pub fn mirror_infra_agent(registry: &OciRegistry, version: &str) -> TestResult<Reference> {
    let source: Reference = format!("docker.io/{INFRA_AGENT_REPOSITORY}:{version}").parse()?;
    mirror_host_package(
        tokio_runtime().handle(),
        &source,
        &publisher(registry, INFRA_AGENT_REPOSITORY),
        version,
    )
}

/// Runs the owner's NRQL and requires a meaningful value, so an aggregate over no data does not pass.
pub fn check_assertion(args: &InstallationArgs, nrql_query: &str) -> TestResult<Vec<Value>> {
    check_query_results(args, nrql_query, |rows| has_meaningful_value(rows))
}

fn has_meaningful_value(rows: &[Value]) -> bool {
    rows.iter().any(|row| {
        row.as_object()
            .is_some_and(|fields| fields.values().any(is_meaningful))
    })
}

fn is_meaningful(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(fields) => !fields.is_empty(),
    }
}

pub fn dump_status() {
    let status = Client::builder()
        .timeout(STATUS_TIMEOUT)
        .build()
        .and_then(|client| client.get(STATUS_URL).send())
        .and_then(|response| response.text());
    match status {
        Ok(status) => info!("AC status endpoint output:\n{status}"),
        Err(err) => warn!(%err, "Could not read the AC status endpoint"),
    }
}
