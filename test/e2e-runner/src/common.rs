use crate::common::agent_metadata::{AgentKind, AgentMetadata, AgentMetadataSet};
use crate::common::nrql::Region;
use std::path::PathBuf;

pub mod agent_metadata;
pub mod cert;
pub mod config;
pub mod docker_hub;
pub mod exec;
pub mod file;
pub mod fleet_control_api;
pub mod health;
pub mod logs;
pub mod nrql;
pub mod oci;
pub mod ohi;
pub mod ohi_agent;
pub mod on_drop;
pub mod runtime;
pub mod self_update;
pub mod test;

/// Common Fleet Control arguments shared across different commands
#[derive(Debug, Clone, clap::Parser)]
pub struct FleetControlArgs {
    /// Fleet ID for Fleet Control tests
    #[arg(long)]
    pub fleet_id: String,

    /// Fleet Control authentication token
    #[arg(long)]
    pub fleet_control_token: String,

    /// Test tags to include when filtering which tests to run (e.g. FLEET_DEPLOYMENT_REMOTE),
    /// can be specified multiple times
    #[arg(long = "include-test-tag")]
    pub include_test_tags: Vec<String>,

    /// Test scenario names to run (e.g. ManagedEntityIsConnectedRemote),
    /// can be specified multiple times
    #[arg(long = "test-scenario")]
    pub test_scenarios: Vec<String>,
}

/// Arguments for scenarios that require Agent Control installation
#[derive(Default, Debug, Clone, clap::Parser)]
pub struct InstallationArgs {
    /// Folder where packages are stored
    #[arg(long)]
    pub artifacts_package_dir: Option<PathBuf>,

    /// Recipes repository
    #[arg(
        long,
        default_value = "https://github.com/newrelic/open-install-library.git"
    )]
    pub recipes_repo: String,

    /// Recipes repository branch
    #[arg(long, default_value = "main")]
    pub recipes_repo_branch: String,

    /// New Relic API key for programmatic access to New Relic services
    #[arg(long)]
    pub nr_api_key: String,

    /// New Relic license key for agent authentication
    #[arg(long)]
    pub nr_license_key: String,

    /// New Relic account identifier for associating the agent
    #[arg(long)]
    pub nr_account_id: String,

    /// System Identity client id
    #[arg(long)]
    pub system_identity_client_id: String,

    /// System Identity private key
    #[arg(long)]
    pub agent_control_private_key: String,

    /// Specific version of agent control to install
    #[arg(long)]
    pub agent_control_version: String,

    /// New Relic region
    #[arg(long, ignore_case = true)]
    pub nr_region: Region,

    /// Agent versions resolved by the `agent-versions` action (its `json` output)
    #[arg(long, env = "AGENT_METADATA")]
    pub agent_metadata: Option<AgentMetadataSet>,

    /// Version of the eBPF agent OCI image to use in tests (not in Instrumentation Metadata yet)
    #[arg(long)]
    pub ebpf_agent_version: Option<String>,

    /// Version of the preload OCI image to use in tests
    #[arg(long)]
    pub preload_version: Option<String>,

    /// Version of the Java APM agent to download from Maven Central in tests
    #[arg(long)]
    pub java_agent_version: Option<String>,

    /// Version of the Ruby APM agent gem to download from rubygems.org in tests
    #[arg(long)]
    pub ruby_agent_version: Option<String>,
}

impl InstallationArgs {
    pub fn agent(&self, agent: AgentKind) -> AgentMetadata {
        self.agent_metadata
            .as_ref()
            .expect("--agent-metadata is required for this scenario")
            .get(agent)
            .clone()
    }
}

/// Arguments for Fleet Control scenarios that also install Agent Control
#[derive(Debug, Clone, clap::Parser)]
pub struct FleetControlInstallationArgs {
    #[command(flatten)]
    pub installation: InstallationArgs,
    #[command(flatten)]
    pub fleet_control: FleetControlArgs,
}

/// Arguments for Fleet Control API tests that don't require Agent Control installation
#[derive(Debug, Clone, clap::Parser)]
pub struct FleetControlApiArgs {
    /// Fleet Control arguments
    #[command(flatten)]
    pub fleet_control: FleetControlArgs,
}

/// Data to set up installation
pub struct RecipeData {
    pub args: InstallationArgs,
    pub fleet_id: String,
    pub fleet_enabled: bool,
    pub recipe_list: String,
    pub proxy_url: String,
}

impl Default for RecipeData {
    fn default() -> Self {
        Self {
            args: Default::default(),
            fleet_id: Default::default(),
            proxy_url: Default::default(),
            fleet_enabled: false,
            recipe_list: "agent-control".to_string(),
        }
    }
}
