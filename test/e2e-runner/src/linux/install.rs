use std::{path::PathBuf, time::Duration};

use tempfile::tempdir;
use tracing::{debug, info, warn};

use crate::{linux::bash::exec_bash_command, tools::test::retry};

/// Arguments to be set for every test that needs Agent Control installation
#[derive(Default, Debug, Clone, clap::Parser)]
pub struct Args {
    /// Folder where '.deb' packages are stored
    #[arg(long)]
    pub deb_package_dir: Option<PathBuf>,

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

    /// Flag to migrate existing infrastructure agent configuration
    #[arg(long, default_value = "true")]
    pub migrate_config_infra: String,
}

/// Data to set up installation
pub struct RecipeData {
    pub args: Args,
    pub fleet_id: String,
    pub fleet_enabled: String,
    pub recipe_list: String,
    pub monitoring_source: String,
    pub proxy_url: String,
}

impl Default for RecipeData {
    fn default() -> Self {
        Self {
            args: Default::default(),
            fleet_id: Default::default(),
            proxy_url: Default::default(),
            fleet_enabled: "false".to_string(),
            recipe_list: "agent-control".to_string(),
            monitoring_source: "infra-agent".to_string(),
        }
    }
}

/// Installs Agent Control using the recipe as configured in the provided [RecipeData].
///
/// It adds a local folder to the trusted repo list. The folder contains the local .deb packages that will be
/// scanned and added to the repo (building the required metadata). After that is done these packages are
/// available to installed with apt.
/// The recipe is still adding the apt upstream production repo so both interoperates, and because of that
/// **the local package must have different from any of the Released ones**.
pub fn install_agent_control_from_recipe(data: &RecipeData) {
    info!("Installing Agent Control from recipe");
    // Set up deb repository
    let repo_dir = tempdir().expect("failed to create temp directory");
    let repo_dir_path = repo_dir.path().display();
    if let Some(deb_package_dir) = data.args.deb_package_dir.as_ref() {
        let deb_package_dir = deb_package_dir.display();
        let repo_command = format!(
            r#"
apt-get install dpkg-dev -y

echo "deb [trusted=yes] file://{repo_dir_path} ./" > /etc/apt/sources.list

cp {deb_package_dir}/*.deb {repo_dir_path}
if [ -z "$(ls -A "{repo_dir_path}")" ]; then
  echo "No packages were found"
  exit 1
fi

cd {repo_dir_path}
dpkg-scanpackages -m . > Packages

apt-get update
"#,
        );
        info!("Setting up local repository");
        debug!("Running command: \n{repo_command}");
        let output = exec_bash_command(&repo_command)
            .unwrap_or_else(|err| panic!("Installation failed: {err}"));
        debug!("Output:\n{output}");
    } else {
        warn!("'deb-package-dir' is not set, skipping local repository setup");
    }

    // Obtain recipes repository
    let recipes_dir = tempdir().expect("failure creating temp dir");
    let recipes_dir_path = recipes_dir.path().display();
    let recipes_repo = data.args.recipes_repo.clone();
    let recipes_branch = data.args.recipes_repo_branch.clone();
    let git_command = format!(
        r"git clone {recipes_repo} --single-branch --branch {recipes_branch} {recipes_dir_path}"
    );
    info!(%recipes_repo, %recipes_branch, "Checking out recipes repo");
    debug!("Running command: \n{git_command}");
    let _ = exec_bash_command(&git_command)
        .unwrap_or_else(|err| panic!("could not checkout recipes repository: {err}"));

    // Install agent control through recipe
    let install_command = format!(
        r#"
curl -Ls https://download.newrelic.com/install/newrelic-cli/scripts/install.sh | \
  bash && sudo \
  NEW_RELIC_CLI_SKIP_CORE=1 \
  NEW_RELIC_LICENSE_KEY={} \
  NEW_RELIC_API_KEY={} \
  NEW_RELIC_ACCOUNT_ID={} \
  NEW_RELIC_AUTH_PROVISIONED_CLIENT_ID={} \
  NEW_RELIC_AUTH_PRIVATE_KEY_PATH={} \
  NEW_RELIC_AGENT_VERSION={} \
  NEW_RELIC_REGION={} \
  NEW_RELIC_AGENT_CONTROL_HOST_MONITORING_SOURCE={} \
  NR_CLI_FLEET_ID={} \
  NEW_RELIC_AGENT_CONTROL_FLEET_ENABLED={} \
  NR_AC_MIGRATE_INFRA_CONFIG={} \
  NEW_RELIC_AGENT_CONTROL=true \
  NEW_RELIC_AGENT_CONTROL_PROXY_URL={} \
  HTTPS_PROXY={} \
  /usr/local/bin/newrelic install \
  -y \
  --localRecipes {}\
  -n {}
"#,
        data.args.nr_license_key,
        data.args.nr_api_key,
        data.args.nr_account_id,
        data.args.system_identity_client_id,
        data.args.agent_control_private_key,
        data.args.agent_control_version,
        data.args.nr_region,
        data.monitoring_source,
        data.fleet_id,
        data.fleet_enabled,
        data.args.migrate_config_infra,
        data.proxy_url,
        data.proxy_url,
        recipes_dir_path,
        data.recipe_list,
    );

    info!("Executing recipe to install Agent Control");
    let output = retry(3, Duration::from_secs(30), "recipe installation", || {
        exec_bash_command(&install_command)
    })
    .unwrap_or_else(|err| panic!("failure executing recipe after retries: {err}"));
    debug!("Output:\n{output}");
}
