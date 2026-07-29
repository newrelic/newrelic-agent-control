use crate::common::config::{DEBUG_LOGGING_CONFIG, update_config, write_agent_local_config};
use crate::common::nrql::{self, Region};
use crate::common::ohi::{
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

pub fn test_nri_redis(args: InstallationArgs) {
    let staging = args.nr_region == Region::Staging;

    let infra_agent_version = args
        .infra_agent_version
        .clone()
        .expect("--infra-agent-version is required for this scenario");

    let redis_version = args
        .redis_version
        .clone()
        .expect("--redis-version is required for this scenario");

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
version: {infra_agent_version}
"#
        ),
    );

    write_agent_local_config(
        &linux::local_config_path("nr-redis"),
        format!(
            r#"
config:
  integrations:
    - name: nri-redis
      env:
        HOSTNAME: 127.0.0.1
        PORT: "6379"
        REMOTE_MONITORING: "true"
      interval: 15s
      labels:
        test.id: {test_id}
version: {redis_version}
"#
        ),
    );

    linux::service::restart_service(linux::SERVICE_NAME);

    let nrql_query =
        format!(r#"SELECT * FROM RedisSample WHERE `label.test.id` = '{test_id}' LIMIT 1"#);
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
