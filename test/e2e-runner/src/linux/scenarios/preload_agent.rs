use crate::common::config::{DEBUG_LOGGING_CONFIG, update_config, write_agent_local_config};
use crate::common::file::write;
use crate::common::on_drop::CleanUp;
use crate::common::test::{TestResult, retry_panic};
use crate::common::{InstallationArgs, RecipeData};
use crate::linux::{
    self,
    bash::exec_bash_command,
    install::{install_agent_control_from_recipe, tear_down_test},
};
use std::path::Path;
use std::time::Duration;
use tracing::{debug, info};
/// Directory where Agent Control loads dynamic (custom) agent type definitions.
const DYNAMIC_AGENT_TYPES_DIR: &str = "/etc/newrelic-agent-control/dynamic-agent-types";
/// World-readable path the preload `.so` is staged to. /var/lib/newrelic-agent-control
/// isn't traversable by non-root, so referencing the OCI-managed copy directly from
/// /etc/ld.so.preload breaks every dynamically linked binary for non-root users.
const STAGED_PRELOAD_SO: &str = "/usr/local/lib/newrelic-preload-agent.so";
/// Expected package installation directory for the preload agent.
fn preload_package_dir() -> String {
    format!(
        "{}/packages/nr-preload/stored_packages/preload-agent",
        linux::AGENT_CONTROL_DATA_DIR
    )
}

pub fn test_installation_with_preload_agent(args: InstallationArgs) {
    let preload_version = args
        .preload_version
        .clone()
        .expect("--preload-version is required for this scenario");
    let recipe_data = RecipeData {
        args,
        ..Default::default()
    };
    let _clean_up = CleanUp::new(tear_down_test);
    install_agent_control_from_recipe(&recipe_data);
    // after the above is done inspect the directory structures to make sure everything is in there
    // particularly the AC local configuration
    let ac_local_config_path = Path::new(&linux::DEFAULT_AC_CONFIG_PATH);
    assert!(
        ac_local_config_path.exists(),
        "AC local config should exist"
    );
    debug!(
        "AC config file contents: {}",
        std::fs::read_to_string(ac_local_config_path).expect("failed to read AC config file")
    );
    let test_id = format!(
        "onhost-e2e-preload-agent_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S%.3f")
    );
    let preload_agent_id = "nr-preload";
    info!("Writing custom preload agent type definition");
    let custom_agent_type_path = format!("{DYNAMIC_AGENT_TYPES_DIR}/preload.yaml");
    let custom_agent_type = r#"namespace: newrelic
name: com.newrelic.preload
version: 0.1.0
variables:
  linux:
    oci:
      repository:
        description: "Package repository name"
        type: string
        required: false
        default: newrelic/preload-agent-artifacts
        variants:
          ac_config_field: "oci_repository_urls"
          values: ["newrelic/preload-agent-artifacts"]
    version:
      description: "Agent version"
      type: string
      required: true
deployment:
  linux:
    packages:
      preload-agent:
        download:
          oci:
            repository: ${nr-var:oci.repository}
            version: ${nr-var:version}
            public_key_url: https://publickeys.newrelic.com/g/agent-control-oci/global/nrpreloadagent/jwks.json
        post_download_hook:
          path: /bin/bash
          args:
            - -c
            - |
              set -euo pipefail
              so_path=$(find "$PACKAGE_DIR" -name '*.so' -print -quit)
              if [ -z "$so_path" ]; then
                echo "No .so file found in extracted preload package at $PACKAGE_DIR" >&2
                exit 1
              fi
              install -m 0644 -o root -g root "$so_path" __STAGED_PRELOAD_SO__
              grep -qxF "__STAGED_PRELOAD_SO__" /etc/ld.so.preload 2>/dev/null \
                || echo "__STAGED_PRELOAD_SO__" >> /etc/ld.so.preload
"#
    .replace("__STAGED_PRELOAD_SO__", STAGED_PRELOAD_SO);
    exec_bash_command(&format!("mkdir -p {DYNAMIC_AGENT_TYPES_DIR}"))
        .unwrap_or_else(|err| panic!("Failed to create dynamic agent types directory: {err}"));
    write(&custom_agent_type_path, &custom_agent_type);
    info!("Setup Agent Control config");
    update_config(
        linux::DEFAULT_AC_CONFIG_PATH,
        format!(
            r#"
host_id: {test_id}
agents:
  {preload_agent_id}:
    agent_type: "newrelic/com.newrelic.preload:0.1.0"
{DEBUG_LOGGING_CONFIG}
"#
        ),
    );
    write_agent_local_config(
        &linux::local_config_path(preload_agent_id),
        format! {r#"
        version: {preload_version}"#},
    );
    linux::service::restart_service(linux::SERVICE_NAME);
    info!("Waiting for preload OCI package to be installed and its post-download hook to apply");
    let package_dir = preload_package_dir();
    let retries = 60;
    retry_panic(
        retries,
        Duration::from_secs(10),
        "preload package installation assertion",
        || assert_preload_agent_installed(&package_dir),
    );
    info!("Test completed successfully");
}

fn assert_preload_agent_installed(package_dir: &str) -> TestResult<()> {
    let output = exec_bash_command(&format!("ls -d {package_dir}"))?;
    if output.contains("No such file") || output.contains("cannot access") {
        return Err(format!("Preload package directory not found yet at {package_dir}").into());
    }
    let listing = exec_bash_command(&format!("ls -la {package_dir}"))?;
    debug!("Package listing:\n{listing}");

    if !Path::new(STAGED_PRELOAD_SO).exists() {
        return Err(
            format!("Staged preload shared library not found yet at {STAGED_PRELOAD_SO}").into(),
        );
    }
    let ld_preload = std::fs::read_to_string("/etc/ld.so.preload")
        .map_err(|err| format!("Failed to read /etc/ld.so.preload: {err}"))?;
    if !ld_preload.lines().any(|line| line.trim() == STAGED_PRELOAD_SO) {
        return Err(format!("{STAGED_PRELOAD_SO} not yet present in /etc/ld.so.preload").into());
    }
    Ok(())
}
