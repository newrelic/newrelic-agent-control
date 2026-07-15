use crate::common::config::{DEBUG_LOGGING_CONFIG, update_config, write_agent_local_config};
use crate::common::nrql::{self, Region};
use crate::common::ohai::{
    EMBEDDED_LINUX_OHI_BINARIES, EMBEDDED_LINUX_OHI_CONFIGS, SHARED_LINUX_FILESYSTEM_DIR,
    check_ohi_shared_filesystem,
};
use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::common::{InstallationArgs, RecipeData};
use crate::linux;
use crate::linux::install::{install_agent_control_from_recipe, tear_down_test};
use crate::linux::redis::Redis;
use std::time::Duration;
use tracing::info;

// Dev OCI packages pre-populated on ghcr.io.
// When production nri-redis OCI ships, delete this scenario's dev-registry wiring and point at the real registry/repos.
const DEV_OCI_REGISTRY: &str = "ghcr.io";
const DEV_INFRA_AGENT_REPO: &str = "newrelic/newrelic-agent-control-infrastructure-dev";
const DEV_INFRA_AGENT_VERSION: &str = "v1.78.0";
const DEV_NRI_REDIS_REPO: &str = "newrelic/newrelic-agent-control-redis-dev";
const DEV_NRI_REDIS_VERSION: &str = "0.0.1";

pub fn test_nri_redis(args: InstallationArgs) {
    let staging = args.nr_region == Region::Staging;

    let test_id = format!(
        "onhost-e2e-nri-redis_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S%.3f"),
    );

    let recipe_data = RecipeData {
        args: args.clone(),
        monitoring_source: "infra-agent".to_string(),
        ..Default::default()
    };
    let _clean_up = CleanUp::new(tear_down_test);
    install_agent_control_from_recipe(&recipe_data);

    let _redis = Redis::start();

    // Point AC at the dev packages on ghcr.io. Signature verification is disabled because the
    // dev packages are not signed with New Relic's production key.
    update_config(
        linux::DEFAULT_AC_CONFIG_PATH,
        format!(
            r#"
host_id: {test_id}
agents:
  nr-infra:
    agent_type: "newrelic/com.newrelic.infrastructure:0.1.0"
  nr-redis:
    agent_type: "newrelic/com.newrelic.infrastructure.nri_redis:0.1.0"
oci:
  registry: {DEV_OCI_REGISTRY}
agent_type_var_constraints:
  variants:
    oci_repository_urls:
      - {DEV_INFRA_AGENT_REPO}
      - {DEV_NRI_REDIS_REPO}
agent_packages:
  signature_verification_enabled: false
{DEBUG_LOGGING_CONFIG}
"#
        ),
    );

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

    write_agent_local_config(
        &linux::local_config_path("nr-redis"),
        format!(
            r#"
config_integration:
  integrations:
    - name: nri-redis
      env:
        HOSTNAME: 127.0.0.1
        PORT: "6379"
        REMOTE_MONITORING: "true"
      interval: 15s
      labels:
        host.id: {test_id}
version: {DEV_NRI_REDIS_VERSION}
"#
        ),
    );

    linux::service::restart_service(linux::SERVICE_NAME);

    let nrql_query =
        format!(r#"SELECT * FROM RedisSample WHERE `label.host.id` = '{test_id}' LIMIT 1"#);
    info!(nrql = nrql_query, "Waiting for RedisSample data in NRDB");
    retry_panic(60, Duration::from_secs(10), "RedisSample NRQL", || {
        nrql::check_query_results_are_not_empty(&recipe_data.args, &nrql_query)
    });

    info!("Verifying shared-filesystem files were populated by AC");
    let expected_binaries = [EMBEDDED_LINUX_OHI_BINARIES.as_slice(), &["nri-redis"]].concat();
    let expected_configs = [EMBEDDED_LINUX_OHI_CONFIGS.as_slice(), &["nri-redis.yaml"]].concat();

    retry_panic(
        30,
        Duration::from_secs(2),
        "shared filesystem OHI binaries and configs",
        || {
            check_ohi_shared_filesystem(
                SHARED_LINUX_FILESYSTEM_DIR,
                &expected_binaries,
                &expected_configs,
            )
        },
    );

    info!("nri-redis Linux scenario completed successfully");
}
