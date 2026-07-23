use crate::common::InstallationArgs;
use crate::common::RecipeData;
use crate::common::config::write_agent_local_config;
use crate::common::nrql::Region;
use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::{
    common::{config, nrql},
    linux::{
        self,
        install::{install_agent_control_from_recipe, tear_down_test},
    },
};
use config::DEBUG_LOGGING_CONFIG;
use std::time::Duration;
use tracing::info;

// Dev OCI packages pre-populated on ghcr.io.
// When production ebpf-agent OCI ships, delete this scenario's dev-registry wiring and point at the real registry/repo.
const DEV_OCI_REGISTRY: &str = "ghcr.io";
const DEV_INFRA_AGENT_REPO: &str = "newrelic/newrelic-agent-control-infrastructure-dev";
const DEV_INFRA_AGENT_VERSION: &str = "v1.78.0";
const DEV_EBPF_AGENT_REPO: &str = "newrelic/newrelic-agent-control-ebpf-dev";

pub fn test_ebpf_agent(args: InstallationArgs) {
    let ebpf_version = args
        .ebpf_agent_version
        .clone()
        .expect("--ebpf-agent-version is required for this scenario");

    let staging = args.nr_region == Region::Staging;

    let recipe_data = RecipeData {
        args,
        monitoring_source: "infra-agent".to_string(),
        recipe_list: "agent-control".to_string(),
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    install_agent_control_from_recipe(&recipe_data);

    let test_id = format!(
        "onhost-e2e-infra-agent_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S%.3f")
    );

    info!("Setup Agent Control config with eBPF");
    // Point AC at the dev packages on ghcr.io. Signature verification is disabled because the
    // dev packages are not signed with New Relic's production key.
    let config = format!(
        r#"
host_id: {test_id}
agents:
  nr-infra:
    agent_type: "newrelic/com.newrelic.infrastructure:0.1.0"
  nr-ebpf:
    agent_type: "newrelic/com.newrelic.ebpf:0.1.0"
oci:
  registry: {DEV_OCI_REGISTRY}
agent_type_var_constraints:
  variants:
    oci_repository_urls:
      - {DEV_INFRA_AGENT_REPO}
      - {DEV_EBPF_AGENT_REPO}
agent_packages:
  signature_verification_enabled: false
{DEBUG_LOGGING_CONFIG}
"#
    );
    config::update_config(linux::DEFAULT_AC_CONFIG_PATH, config);
    // eBPF agent config
    let region = if staging { "staging" } else { "US" };
    let ebpf_config = format!(
        r#"
config:
  deploymentName: "{test_id}"
  region: "{region}"
oci:
  repository: {DEV_EBPF_AGENT_REPO}
version: "{ebpf_version}"
    "#
    );
    write_agent_local_config(&linux::local_config_path("nr-ebpf"), ebpf_config);
    // Infra agent config: it is used to generate traffic for eBPF metrics to appear
    write_agent_local_config(
        &linux::local_config_path("nr-infra"),
        format!(
            r#"
config_agent:
  license_key: '{{{{NEW_RELIC_LICENSE_KEY}}}}'
  staging: {staging}
version: {DEV_INFRA_AGENT_VERSION}
oci:
  repository: {DEV_INFRA_AGENT_REPO}
"#
        ),
    );

    linux::service::restart_service(linux::SERVICE_NAME);

    let nrql_query = format!(
        r#"SELECT * FROM Metric WHERE metricName = 'ebpf.tcp.connection_duration' AND deployment.name = '{test_id}' LIMIT 1"#
    );
    info!(nrql = nrql_query, "Checking results of NRQL");
    let retries = 60;
    retry_panic(retries, Duration::from_secs(10), "nrql assertion", || {
        nrql::check_query_results_are_not_empty(&recipe_data.args, &nrql_query)
    });
}
