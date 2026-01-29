use std::path::PathBuf;

pub mod config;
pub mod exec;
pub mod file;
pub mod logs;
pub mod nrql;
pub mod on_drop;
pub mod test;

/// Arguments to be set for every test that needs Agent Control installation
#[derive(Default, Debug, Clone, clap::Parser)]
pub struct Args {
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

    /// Flag to migrate existing infrastructure agent configuration. Currently used only in Linux.
    #[arg(long, default_value = "true")]
    pub migrate_config_infra: String,
}

/// Data to set up installation
pub struct RecipeData {
    pub args: Args,
    pub fleet_id: String,
    pub fleet_enabled: String,
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
            fleet_enabled: "false".to_string(),
            recipe_list: "agent-control".to_string(),
            #[cfg(target_family = "unix")]
            monitoring_source: "infra-agent".to_string(),
            #[cfg(target_family = "windows")]
            monitoring_source: "".to_string(),
        }
    }
}
