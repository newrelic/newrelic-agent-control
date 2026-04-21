use std::path::PathBuf;

pub mod config;
pub mod exec;
pub mod file;
pub mod fleet_control_api;
pub mod logs;
pub mod nrql;
pub mod on_drop;
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

    /// Fleet type for Fleet Control API (e.g. linux-fleet or k8s-fleet)
    #[arg(long)]
    pub fleet_type: String,

    /// Name of the test suite to run (e.g. DeploymentServicesTestSuite)
    #[arg(long)]
    pub test_suite: String,
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
    #[arg(long, default_value = "US")]
    pub nr_region: String,

    /// Version of the infrastructure agent OCI image to use in tests
    #[arg(long)]
    pub infra_agent_version: Option<String>,

    /// Version of the NRDot OCI image to use in tests
    #[arg(long)]
    pub nrdot_version: Option<String>,

    /// Fleet Control arguments
    #[command(flatten)]
    pub fleet_control: Option<FleetControlArgs>,
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
    pub monitoring_source: String,
}

impl Default for RecipeData {
    fn default() -> Self {
        Self {
            args: Default::default(),
            fleet_id: Default::default(),
            proxy_url: Default::default(),
            fleet_enabled: false,
            recipe_list: "agent-control".to_string(),
            monitoring_source: "infra-agent".to_string(),
        }
    }
}
