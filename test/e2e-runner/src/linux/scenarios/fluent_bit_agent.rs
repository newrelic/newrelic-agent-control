use crate::common::config::{
    DEBUG_LOGGING_CONFIG, append_to_config_file, update_config, write_agent_local_config,
};
use crate::common::fluent_bit_package::build_fluentbit_package;
use crate::common::nrql::{self, Region};
use crate::common::oci::{OciRegistry, push_and_sign};
use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::common::{InstallationArgs, RecipeData};
use crate::linux;
use crate::linux::install::{install_agent_control_from_recipe, tear_down_test};
use crate::linux::service::{STATUS_RUNNING, restart_service_and_wait};
use oci_test_utils::PackageMediaType;
use std::fs;
use std::path::Path;
use std::time::Duration;
use tracing::info;

const AGENT_ID: &str = "nr-logging";
const AGENT_TYPE: &str = "newrelic/io.fluentbit:0.1.0";
const AGENT_TYPE_FILE: &str =
    "/etc/newrelic-agent-control/dynamic-agent-types/io_fluentbit_e2e.yaml";
const ENV_VARS_FILE: &str = "/etc/newrelic-agent-control/environment_variables.yaml";
const HEALTH_PORT: u16 = 2020;

// Proves Agent Control can run a self-contained, standalone Fluent Bit as its own on-host
// sub-agent -- with no Infrastructure agent installed -- forwarding records to New Relic.
// The sub-agent is driven by pure local config (no OpAMP); the io.fluentbit agent type is
// registered dynamically rather than added to the embedded registry.
pub fn test_installation_with_standalone_fluent_bit(args: InstallationArgs) {
    let staging = args.nr_region == Region::Staging;
    let license_key = args.nr_license_key.clone();

    let recipe_data = RecipeData {
        args,
        fleet_enabled: false,
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    // Only the `agent-control` recipe is installed (the default `recipe_list`): no infra agent
    // ever touches this host.
    install_agent_control_from_recipe(&recipe_data);

    let registry = OciRegistry::start();

    info!("Building the standalone Fluent Bit OCI package");
    let (_package_dir, archive) = build_fluentbit_package();
    let pushed = push_and_sign(&archive, PackageMediaType::TarGz);
    let version = pushed.reference.tag().unwrap();

    let test_id = format!(
        "onhost-e2e-fluentbit_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S%.3f")
    );

    info!("Writing the dynamic io.fluentbit agent type");
    write_agent_type(&pushed.jwks_url);

    info!("Setting up Agent Control config");
    update_config(
        linux::DEFAULT_AC_CONFIG_PATH,
        format!(
            r#"
oci:
  registry: {}
agents:
  {AGENT_ID}:
    agent_type: "{AGENT_TYPE}"
{DEBUG_LOGGING_CONFIG}
"#,
            registry.url()
        ),
    );

    info!("Injecting NR_LICENSE_KEY into Agent Control's own environment");
    append_to_config_file(ENV_VARS_FILE, &format!("NR_LICENSE_KEY: {license_key}"));

    let endpoint_directive = if staging {
        "    endpoint https://staging-log-api.newrelic.com/log/v1\n"
    } else {
        ""
    };
    let config_fluentbit = format!(
        r#"[SERVICE]
    Flush 1
    Daemon Off
    HTTP_Server On
    HTTP_Listen 0.0.0.0
    HTTP_PORT {HEALTH_PORT}
    Health_Check On

[INPUT]
    Name dummy
    Tag e2e.fluentbit
    Dummy {{"host.id":"{test_id}"}}

[OUTPUT]
    Name newrelic
    Match *
    licenseKey ${{NR_LICENSE_KEY}}
{endpoint_directive}"#
    );

    write_agent_local_config(
        &linux::local_config_path(AGENT_ID),
        format!(
            r#"
version: '{version}'
health_port: {HEALTH_PORT}
config_fluentbit: |
{indented}
"#,
            indented = indent_block(&config_fluentbit, "  "),
        ),
    );

    restart_service_and_wait(linux::SERVICE_NAME, STATUS_RUNNING);

    info!("Waiting for Fluent Bit's health endpoint to report healthy");
    let health_url = format!("http://localhost:{HEALTH_PORT}/api/v1/health");
    retry_panic(
        30,
        Duration::from_secs(2),
        "fluent-bit health endpoint",
        || {
            reqwest::blocking::get(&health_url)
                .and_then(|r| r.error_for_status())
                .map(|_| ())
                .map_err(|e| e.into())
        },
    );

    let nrql_query = format!(r#"SELECT * FROM Log WHERE `host.id` = '{test_id}' LIMIT 1"#);
    info!(nrql = nrql_query, "Checking results of NRQL to check logs");
    retry_panic(30, Duration::from_secs(10), "nrql assertion", || {
        nrql::check_query_results_are_not_empty(&recipe_data.args, &nrql_query)
    });

    info!("Standalone Fluent Bit e2e scenario completed successfully");
}

// Writes the io.fluentbit agent type definition to the host's dynamic-agent-types directory.
// The OCI package repository/version stay as `${nr-var:...}` (supplied by the sub-agent's local
// config); only the JWKS URL, which is unique per test run, is interpolated here.
fn write_agent_type(jwks_url: &str) {
    let yaml = format!(
        r#"
namespace: newrelic
name: io.fluentbit
version: 0.1.0
platform: host
operating_system: linux
protocol_version: "1.0"
variables:
  config_fluentbit:
    description: "fb config"
    type: string
    required: true
  version:
    description: "fb version"
    type: string
    required: false
    default: latest
deployment:
  health:
    interval: 30s
    initial_delay: 10s
    timeout: 5s
    http:
      path: /api/v1/health
      port: {HEALTH_PORT}
  packages:
    fluent-bit:
      download:
        oci:
          repository: test
          version: ${{nr-var:version}}
  filesystem:
    config:
      kind: dir
      entries:
        fluent-bit.conf:
          kind: file
          text: ${{nr-var:config_fluentbit}}
  executables:
    - id: fluent-bit
      path: ${{nr-sub:packages.fluent-bit.dir}}/fluent-bit
      args:
        - -c
        - ${{nr-sub:filesystem_agent_dir}}/config/fluent-bit.conf
        - -e
        - ${{nr-sub:packages.fluent-bit.dir}}/out_newrelic.so
        - -R
        - ${{nr-sub:packages.fluent-bit.dir}}/parsers.conf
      env:
        NR_LICENSE_KEY: "${{nr-env:NR_LICENSE_KEY}}"
"#
    );

    let path = Path::new(AGENT_TYPE_FILE);
    fs::create_dir_all(path.parent().unwrap())
        .expect("failed to create dynamic-agent-types directory");
    fs::write(path, yaml).expect("failed to write the io.fluentbit agent type");
}

fn indent_block(text: &str, prefix: &str) -> String {
    text.lines()
        .map(|line| format!("{prefix}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}
