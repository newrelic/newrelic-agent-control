use crate::common::config::{DEBUG_LOGGING_CONFIG, update_config, write_agent_local_config};
use crate::common::http_stub::JsonStub;
use crate::common::nrql;
use crate::common::ohi::{
    EMBEDDED_WINDOWS_OHI_BINARIES, SHARED_WINDOWS_FILESYSTEM_DIR, check_ohi_shared_filesystem,
};
use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::common::{InstallationArgs, RecipeData};
use crate::windows;
use crate::windows::install::{SERVICE_NAME, install_agent_control_from_recipe, tear_down_test};
use crate::windows::service::{STATUS_RUNNING, restart_service};
use std::time::Duration;
use tracing::info;

const STUB_BODY: &str = r#"{"value": 42}"#;

// TODO: Remove once flex is published
// Dev OCI packages pre-populated on ghcr.io.
// When production nri-flex OCI ships, delete this scenario's dev-registry wiring and point at the real registry/repos.
const DEV_OCI_REGISTRY: &str = "ghcr.io";
const DEV_INFRA_AGENT_REPO: &str = "newrelic/newrelic-agent-control-infrastructure-dev";
const DEV_INFRA_AGENT_VERSION: &str = "v1.78.0";
const DEV_NRI_FLEX_REPO: &str = "newrelic/newrelic-agent-control-flex-dev";

pub fn test_nri_flex(args: InstallationArgs) {
    let flex_version = args
        .flex_version
        .clone()
        .expect("--flex-version is required for this scenario");

    let test_id = format!(
        "onhost-e2e-nri-flex_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S%.3f"),
    );

    let recipe_data = RecipeData {
        args: args.clone(),
        ..Default::default()
    };
    let _clean_up = CleanUp::new(tear_down_test);
    install_agent_control_from_recipe(&recipe_data);

    let stub = JsonStub::start(STUB_BODY);

    update_config(
        windows::DEFAULT_AC_CONFIG_PATH,
        format!(
            r#"
host_id: {test_id}
agents:
  nr-infra:
    agent_type: "newrelic/com.newrelic.infrastructure:0.1.0"
  nr-flex:
    agent_type: "newrelic/com.newrelic.infrastructure.nri_flex:0.1.0"
oci:
  registry: {DEV_OCI_REGISTRY}
agent_type_var_constraints:
  variants:
    oci_repository_urls:
      - {DEV_INFRA_AGENT_REPO}
      - {DEV_NRI_FLEX_REPO}
agent_packages:
  signature_verification_enabled: false
{DEBUG_LOGGING_CONFIG}
"#
        ),
    );

    write_agent_local_config(
        &windows::local_config_path("nr-infra"),
        format!(
            r#"
config_agent:
  license_key: '{{{{NEW_RELIC_LICENSE_KEY}}}}'
  log:
    level: debug
version: {DEV_INFRA_AGENT_VERSION}
oci:
  repository: {DEV_INFRA_AGENT_REPO}
"#
        ),
    );

    let stub_port = stub.port();
    write_agent_local_config(
        &windows::local_config_path("nr-flex"),
        format!(
            r#"
config:
  integrations:
    - name: nri-flex
      interval: 15s
      config:
        name: e2e-flex
        apis:
          - event_type: FlexE2ESample
            url: http://127.0.0.1:{stub_port}/metrics
            custom_attributes:
              test.id: {test_id}
version: {flex_version}
oci:
  repository: {DEV_NRI_FLEX_REPO}
"#
        ),
    );

    restart_service(SERVICE_NAME, STATUS_RUNNING);

    let nrql_query =
        format!(r#"SELECT * FROM FlexE2ESample WHERE `test.id` = '{test_id}' LIMIT 1"#);
    info!(nrql = nrql_query, "Waiting for FlexE2ESample data in NRDB");
    retry_panic(60, Duration::from_secs(10), "FlexE2ESample NRQL", || {
        nrql::check_query_results_are_not_empty(&recipe_data.args, &nrql_query)
    });

    info!("Verifying shared-filesystem files were populated by AC");
    let expected_binaries = [EMBEDDED_WINDOWS_OHI_BINARIES.as_slice(), &["nri-flex.exe"]].concat();
    let expected_configs = ["nri-flex.yaml"].as_slice();

    retry_panic(
        30,
        Duration::from_secs(2),
        "shared filesystem OHI binaries and configs",
        || {
            check_ohi_shared_filesystem(
                SHARED_WINDOWS_FILESYSTEM_DIR,
                &expected_binaries,
                expected_configs,
            )
        },
    );

    info!("nri-flex Windows scenario completed successfully");
}
