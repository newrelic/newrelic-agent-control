use std::{path::PathBuf, time::Duration};

use tempfile::tempdir;
use tracing::info;

use crate::{
    linux::bash::exec_bash_command,
    tools::test::{TestResult, retry},
};

#[derive(Default)] // TODO: proper default
pub struct RecipeInstallationData {
    deb_package_dir: PathBuf,
    recipes_repo: String,
    recipes_repo_branch: String,

    nr_api_key: String,
    nr_license_key: String,
    nr_account_id: String,
    system_identity_client_id: String,
    agent_control_private_key: String,
    agent_control_version: String,
    nr_region: String,
    monitoring_source: String,
    migrate_config_infra: String,
    fleet_id: String,
    fleet_enabled: String,
    ac_proxy_url: String,
    recipe_list: String,
}

pub fn install_agent_control_from_recipe(args: RecipeInstallationData) -> TestResult<()> {
    // Set up deb repository
    let repo_dir = tempdir()?;
    let repo_dir_path = repo_dir.path().display();
    let deb_package_dir = args.deb_package_dir.display();
    let repo_command = format!(
        r#"
apt-get install dpkg-dev -y

echo "deb [trusted=yes] file://{repo_dir_path} ./" >> /etc/apt/sources.list

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
    info!("Set up local repository");
    let output = exec_bash_command(&repo_command, "could not set up local repository")?;
    println!("Output:\n---{output}\n---");

    // Obtain recipes repocitory
    let recipes_dir = tempdir()?;
    let recipes_dir_path = recipes_dir.path().display();
    let recipes_repo = args.recipes_repo;
    let recipes_branch = args.recipes_repo_branch;
    let git_command = format!(
        r"git clone {recipes_repo} --single-branch --branch {recipes_branch} {recipes_dir_path}"
    );
    info!(%recipes_repo, %recipes_branch, "Checkout recipes repo");
    let output = exec_bash_command(&git_command, "could not clone recipes repository")?;
    println!("Output:\n---{output}\n---");

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
        args.nr_license_key,
        args.nr_api_key,
        args.nr_account_id,
        args.system_identity_client_id,
        args.agent_control_private_key,
        args.agent_control_version,
        args.nr_region,
        args.monitoring_source,
        args.fleet_id,
        args.fleet_enabled,
        args.migrate_config_infra,
        args.ac_proxy_url,
        args.ac_proxy_url,
        recipes_dir_path,
        args.recipe_list,
    );

    let output = retry(3, Duration::from_secs(30), "recipe installation", || {
        exec_bash_command(&install_command, "error installing with the recipe")
    })?;
    println!("Output:\n---\n{output}\n---");

    Ok(())
}
