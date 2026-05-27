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
use std::time::Duration;
use tracing::{debug, info};

/// Directory where Agent Control loads dynamic (custom) agent type definitions.
const DYNAMIC_AGENT_TYPES_DIR: &str = "/etc/newrelic-agent-control/dynamic-agent-types";

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
        monitoring_source: "preload-agent".to_string(),
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    install_agent_control_from_recipe(&recipe_data);

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
"#;
    exec_bash_command(&format!("mkdir -p {DYNAMIC_AGENT_TYPES_DIR}"))
        .unwrap_or_else(|err| panic!("Failed to create dynamic agent types directory: {err}"));
    write(&custom_agent_type_path, custom_agent_type);

    info!("Setup Agent Control config");
    update_config(
        linux::DEFAULT_AC_CONFIG_PATH,
        format!(
            r#"
host_id: {test_id}
agents:
  nr-preload:
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

    info!("Waiting for preload OCI package to be downloaded and extracted");
    let package_dir = preload_package_dir();
    let retries = 60;
    retry_panic(
        retries,
        Duration::from_secs(10),
        "preload package download assertion",
        || assert_preload_package_downloaded(&package_dir),
    );

    info!("Searching for shared library inside extracted package");
    let find_so_command = format!(r#"find {package_dir} -type f -name "*.so" | head -n 1"#);
    let so_path = exec_bash_command(&find_so_command)
        .unwrap_or_else(|err| panic!("Failed to find shared library in package: {err}"));
    let so_path = so_path.lines().last().unwrap_or("").trim().to_string();
    if so_path.is_empty() {
        panic!("No .so file found in extracted preload package at {package_dir}");
    }
    info!("Found shared library: {so_path}");

    info!("Installing shared library into /etc/ld.so.preload");
    let install_command = format!(r#"echo "{so_path}" >> /etc/ld.so.preload"#);
    let output = exec_bash_command(&install_command)
        .unwrap_or_else(|err| panic!("Editing /etc/ld.so.preload failed: {err}"));
    debug!("Install output:\n{output}");

    info!("Test completed successfully");
}

fn assert_preload_package_downloaded(package_dir: &str) -> TestResult<()> {
    let output = exec_bash_command(&format!("ls -d {package_dir}"))?;
    if output.contains("No such file") || output.contains("cannot access") {
        return Err(format!("Preload package directory not found yet at {package_dir}").into());
    }
    let listing = exec_bash_command(&format!("ls -la {package_dir}"))?;
    debug!("Package listing:\n{listing}");
    Ok(())
}
