use std::{fs, path::PathBuf, process::Command, time::Duration};

use tempfile::tempdir;
use tracing::{debug, info};

use crate::{
    tools::test::retry,
    windows::powershell::{exec_powershell_cmd, exec_powershell_command},
};

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

/// Data to set up installation
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

/// Installs Agent Control using the recipe as configured in the provided [RecipeData].
///
///
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
    debug!(
        "Installing newrelic cli with command: \n{}",
        install_newrelic_cli_command
    );
    let _ = exec_powershell_command(install_newrelic_cli_command)
        .unwrap_or_else(|err| panic!("could not install New Relic CLI: {err}"));

    // By default, the windows recipe will download the zip file from https://download.newrelic.com and put it
    // in "$env:TEMP\newrelic-agent-control.zip". If the zip file already exists, the recipe will skip the
    // download. We can take advantage of this behavior by placing our zip file in the expected location.
    // That way we avoid trying to download the artifact from the wrong place.
    if let Some(path) = &data.args.artifacts_package_dir {
        debug!(
            "Using local artifacts package directory: {}",
            path.display()
        );

        let zip_name = PathBuf::from(path).join(format!(
            "newrelic-agent-control_{}_windows_amd64.zip",
            data.args.agent_control_version
        ));
        let extract_path = format!(
            "$env:TEMP/newrelic-agent-control-{}.zip",
            data.args.agent_control_version
        );

        debug!(
            "Copying zip from \"{}\" to \"{}\"",
            zip_name.display(),
            extract_path
        );
        let copy_zip_command = format!("cp {} {}", zip_name.display(), extract_path);
        debug!("{copy_zip_command}");

        let _ = exec_powershell_command(&copy_zip_command)
            .unwrap_or_else(|err| panic!("could not copy zip: {err}"));
    }

    // Install agent control through recipe
    // We need to use 2>&1 to redirect stderr to stdout for debugging.
    // Otherwise, newrelic cli won't show any error messages.
    let install_command = format!(
        r#"
$env:NEW_RELIC_CLI_SKIP_CORE='1'; `
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
$env:NEW_RELIC_AGENT_CONTROL_SKIP_BINARY_SIGNATURE_VALIDATION='true'; `
& "C:\Program Files\New Relic\New Relic CLI\newrelic.exe" install `
-y `
--localRecipes {} `
-n {} `
--debug 2>&1
"#,
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
    debug!("Install command: \n{install_command}\n");

    info!("Executing recipe to install Agent Control");
    // Create a temporary .ps1 file for the installation command
    //
    // There's an option that allows running commands directly. That is "-Command". The
    // issue with that is that "-ExecutionPolicy" won't bypass all the checks. It seems
    // to only work properly using a ps1 script. We are forced to create a temporary script file.
    debug!("Creating install script");
    let script_dir = tempdir().expect("failed to create temp dir for script");
    let script_path = script_dir.path().join("install_command.ps1");
    fs::write(&script_path, &install_command)
        .unwrap_or_else(|err| panic!("failed to write install script: {err}"));

    debug!("Executing install script: {}", script_path.display());
    let output = retry(3, Duration::from_secs(30), "recipe installation", || {
        let mut cmd = Command::new("powershell.exe");
        let cmd = cmd
            .current_dir(script_dir.path())
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(&script_path);

        exec_powershell_cmd(cmd)
    })
    .unwrap_or_else(|err| panic!("failure executing recipe after retries: {err}"));

    debug!("Output:\n{output}");
}
