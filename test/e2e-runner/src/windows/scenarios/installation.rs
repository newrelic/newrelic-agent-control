use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use crate::tools::logs::ShowLogsOnDrop;
use crate::tools::test::retry;
use crate::windows;
use crate::{tools::config, windows::powershell::exec_powershell_command};
use tempfile::tempdir;
use tracing::{debug, info};

const DEFAULT_STATUS_PORT: u16 = 51200;
const SERVICE_NAME: &str = "newrelic-agent-control";

/// Arguments to be set for every test that needs Agent Control installation
#[derive(Default, Debug, clap::Parser)]
pub struct Args {
    /// Folder where '.deb' packages are stored
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

    /// New Relic license key for agent authentication
    #[arg(long)]
    pub nr_license_key: String,

    /// New Relic API key for programmatic access to New Relic services
    #[arg(long)]
    pub nr_api_key: String,

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
}

pub struct RecipeData {
    pub args: Args,
    pub fleet_id: String,
    pub fleet_enabled: String,
    pub recipe_list: String,
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
        }
    }
}

/// Runs a complete Windows E2E installation test.
pub fn test_installation(args: Args) {
    let recipe_data = RecipeData {
        args,
        ..Default::default()
    };
    install_agent_control_from_recipe(&recipe_data);

    info!("Waiting 10 seconds for service to start");
    thread::sleep(Duration::from_secs(10));

    windows::service::check_service_running(SERVICE_NAME).expect("service should be running");

    config::update_config_for_debug_logging(
        windows::DEFAULT_CONFIG_PATH,
        windows::DEFAULT_LOG_PATH,
    );

    windows::service::restart_service(SERVICE_NAME);
    info!("Waiting 10 seconds for service to start");
    thread::sleep(Duration::from_secs(10));

    let _show_logs = ShowLogsOnDrop::from(windows::DEFAULT_CONFIG_PATH);

    info!("Verifying service health");
    let status_endpoint = format!("http://localhost:{DEFAULT_STATUS_PORT}/status");
    let status = retry(30, Duration::from_secs(2), "health check", || {
        windows::health::check_health(&status_endpoint)
    })
    .unwrap(); // TODO
    info!("Agent Control is healthy");
    let status_json = serde_json::to_string_pretty(&status).unwrap(); // TODO
    info!(response = status_json, "Agent Control is healthy");

    windows::cleanup::cleanup(SERVICE_NAME);
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
    let _ = exec_powershell_command(&git_command)
        .unwrap_or_else(|err| panic!("could not checkout recipes repository: {err}"));

    let install_newrelic_cli_command = r#"
(New-Object System.Net.WebClient).DownloadFile("https://github.com/newrelic/newrelic-cli/releases/latest/download/NewRelicCLIInstaller.msi", "$env:TEMP\NewRelicCLIInstaller.msi"); `
msiexec.exe /qn /i "$env:TEMP\NewRelicCLIInstaller.msi" | Out-Null;
"#;
    let _ = exec_powershell_command(&install_newrelic_cli_command)
        .unwrap_or_else(|err| panic!("could not install New Relic CLI: {err}"));

    let mut download_url = String::new();
    if let Some(path) = &data.args.artifacts_package_dir {
        download_url = format!(
            "$env:NEW_RELIC_DOWNLOAD_URL='file:///{}/newrelic-agent-control_{}_windows_amd64.zip#'; `",
            path.display(), data.args.agent_control_version
        );
    }

    // Install agent control through recipe
    let install_command = format!(
        r#"
{}$env:NEW_RELIC_CLI_SKIP_CORE='1'; `
$env:NEW_RELIC_LICENSE_KEY='{}'; `
$env:NEW_RELIC_API_KEY='{}'; `
$env:NEW_RELIC_ACCOUNT_ID='{}'; `
$env:NEW_RELIC_AUTH_PROVISIONED_CLIENT_ID='{}'; `
$env:NEW_RELIC_AUTH_PRIVATE_KEY_PATH='{}'; `
$env:NEW_RELIC_AGENT_VERSION='{}'; `
$env:NEW_RELIC_REGION='{}'; `
$env:NR_CLI_FLEET_ID='{}'; `
$env:NEW_RELIC_AGENT_CONTROL_FLEET_ENABLED='{}'; `
$env:NEW_RELIC_AGENT_CONTROL='true'; `
$env:NEW_RELIC_AGENT_CONTROL_PROXY_URL='{}'; `
$env:HTTPS_PROXY='{}'; `
& "C:\Program Files\New Relic\New Relic CLI\newrelic.exe" install `
-y `
--localRecipes {} `
-n {}
"#,
        download_url,
        data.args.nr_license_key,
        data.args.nr_api_key,
        data.args.nr_account_id,
        data.args.system_identity_client_id,
        data.args.agent_control_private_key,
        data.args.agent_control_version,
        data.args.nr_region,
        data.fleet_id,
        data.fleet_enabled,
        data.proxy_url,
        data.proxy_url,
        recipes_dir_path,
        data.recipe_list,
    );

    info!("Executing recipe to install Agent Control");
    let output = retry(3, Duration::from_secs(30), "recipe installation", || {
        exec_powershell_command(&install_command)
    })
    .unwrap_or_else(|err| panic!("failure executing recipe after retries: {err}"));
    debug!("Output:\n{output}");
}
