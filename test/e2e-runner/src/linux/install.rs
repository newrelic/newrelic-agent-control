use crate::common::RecipeData;
use crate::common::logs::show_logs;
use crate::linux::DEFAULT_LOG_PATH;
use crate::{common::test::retry, linux::bash::exec_bash_command};
use std::time::Duration;
use tempfile::tempdir;
use tracing::{debug, info, warn};

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
    if let Some(deb_package_dir) = data.args.artifacts_package_dir.as_ref() {
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

apt-get update || true
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

    // Build the recipe flag: -c <file> takes priority over --localRecipes + -n
    let recipe_flag = if let Some(recipe_file) = data.args.recipe_file.as_ref() {
        info!(path = %recipe_file.display(), "Using local recipe file (-c)");
        format!("-c {}", recipe_file.display())
    } else {
        // Obtain recipes repository — use local path directly if provided, otherwise git clone
        let recipes_dir_owned;
        let recipes_dir_path: String = if let Some(local) = data.args.local_recipes_dir.as_ref() {
            info!(path = %local.display(), "Using local recipes directory (skipping git clone)");
            local.display().to_string()
        } else {
            recipes_dir_owned = tempdir().expect("failure creating temp dir");
            let path = recipes_dir_owned.path().display().to_string();
            let recipes_repo = data.args.recipes_repo.clone();
            let recipes_branch = data.args.recipes_repo_branch.clone();
            let git_command = format!(
                r"git clone {recipes_repo} --single-branch --branch {recipes_branch} {path}"
            );
            info!(%recipes_repo, %recipes_branch, "Checking out recipes repo");
            debug!("Running command: \n{git_command}");
            let _ = exec_bash_command(&git_command)
                .unwrap_or_else(|err| panic!("could not checkout recipes repository: {err}"));
            path
        };
        format!("--localRecipes {} -n {}", recipes_dir_path, data.recipe_list)
    };

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
  NEW_RELIC_AGENT_CONTROL=true \
  NEW_RELIC_AGENT_CONTROL_PROXY_URL={} \
  HTTPS_PROXY={} \
  /usr/local/bin/newrelic install \
  {}
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
        data.proxy_url,
        data.proxy_url,
        recipe_flag,
    );

    info!("Executing recipe to install Agent Control");
    let output = retry(3, Duration::from_secs(30), "recipe installation", || {
        exec_bash_command(&install_command)
    })
    .unwrap_or_else(|err| panic!("failure executing recipe after retries: {err}"));
    info!("Output:\n{output}");

    // If the recipe did not install the binary (e.g. "not supported" on this platform),
    // fall back to a direct apt-get install.
    let binary_path = "/usr/bin/newrelic-agent-control";
    if !std::path::Path::new(binary_path).exists() {
        if data.args.artifacts_package_dir.is_some() {
            // Local .deb repo is still mounted — install from it directly.
            warn!(
                "Recipe did not install '{}'. Falling back to direct apt-get install.",
                binary_path
            );
            let fallback = exec_bash_command(
                "apt-get install -y --allow-downgrades newrelic-agent-control && systemctl daemon-reload",
            );
            match fallback {
                Ok(out) => info!("Direct install output:\n{out}"),
                Err(err) => warn!("Direct install failed: {err}"),
            }
        } else {
            // No local package dir — download the .deb directly from the S3 pool and install
            // with dpkg. The apt repo metadata may not exist for all distro codenames, but
            // the pool URL follows a predictable pattern.
            warn!(
                "Recipe did not install '{}'. Falling back to direct .deb download from S3.",
                binary_path
            );
            let version = &data.args.agent_control_version;
            let download_script = format!(
                r#"
ARCH=$(dpkg --print-architecture)
DEB_URL="http://nr-downloads-ohai-testing.s3-website-us-east-1.amazonaws.com/poc_selfupdate/linux/apt/pool/main/n/newrelic-agent-control/newrelic-agent-control_{version}_${{ARCH}}.deb"
DEB_FILE=$(mktemp /tmp/newrelic-agent-control-XXXXXX.deb)
echo "Downloading $DEB_URL"
curl -fsSL "$DEB_URL" -o "$DEB_FILE"
apt-get install -y "$DEB_FILE"
rm -f "$DEB_FILE"
systemctl daemon-reload
"#
            );
            match exec_bash_command(&download_script) {
                Ok(out) => info!("Direct .deb install output:\n{out}"),
                Err(err) => warn!("Direct .deb install failed: {err}"),
            }
        }
        if !std::path::Path::new(binary_path).exists() {
            panic!(
                "Agent Control binary '{}' was not installed. \
                 Provide a working recipe (--recipe-file) or a .deb package (--artifacts-package-dir).",
                binary_path
            );
        }
    }
}

pub fn tear_down_test() {
    let _ = show_logs(DEFAULT_LOG_PATH);
    if let Ok(output) = crate::linux::bash::exec_bash_command(
        "journalctl -u newrelic-agent-control --no-pager -n 80 --output=short-precise",
    ) {
        tracing::info!("journalctl (last 80 lines):\n{output}");
    }
}
