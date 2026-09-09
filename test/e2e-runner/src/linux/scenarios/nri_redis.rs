use crate::common::config::{DEBUG_LOGGING_CONFIG, update_config, write_agent_local_config};
use crate::common::logs::show_logs;
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

/// Newrelic-infra's own debug log, written by the infra-agent via `config_agent.log.file`.
/// Colocated with AC's own logs so on-host operators find both in one place, and root-writable
/// because AC already creates and manages the parent directory.
const INFRA_AGENT_LOG_PATH: &str = "/var/log/newrelic-agent-control/newrelic-infra.log";

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
        ..Default::default()
    };
    let _clean_up = CleanUp::new(tear_down_test);
    // Drops in reverse declaration order — this runs before `_clean_up` so the infra-agent
    // log lands next to (not after) the AC logs on failure or success.
    let _infra_log_dump = CleanUp::new(|| {
        let _ = show_logs(INFRA_AGENT_LOG_PATH);
    });
    install_agent_control_from_recipe(&recipe_data);

    let _redis = Redis::start();

    // Phase 1: install nr-infra only, and wait until it is reporting SystemSample.
    // This ensures newrelic-infra is up and consuming the shared-filesystem OHI configs
    // *before* nr-redis writes its config and binary — exercising the "add OHI to a
    // running infra-agent" path rather than the cold-start path.
    info!("Phase 1: installing nr-infra only");
    update_config(
        linux::DEFAULT_AC_CONFIG_PATH,
        format!(
            r#"
host_id: {test_id}
agents:
  nr-infra:
    agent_type: "newrelic/com.newrelic.infrastructure:0.1.0"
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
  log:
    level: debug
    file: {INFRA_AGENT_LOG_PATH}
version: {infra_agent_version}
"#
        ),
    );

    linux::service::restart_service(linux::SERVICE_NAME);

    let system_sample_query =
        format!(r#"SELECT * FROM SystemSample WHERE `host.id` = '{test_id}' LIMIT 1"#);
    info!(
        nrql = system_sample_query,
        "Waiting for SystemSample data in NRDB (nr-infra steady state)"
    );
    retry_panic(60, Duration::from_secs(10), "SystemSample NRQL", || {
        nrql::check_query_results_are_not_empty(&recipe_data.args, &system_sample_query)
    });

    // Phase 2: add nr-redis. `update_config` merges only at the top level, so the
    // `agents` map is replaced wholesale — both agents must be listed here.
    info!("Phase 2: adding nr-redis to the running AC");
    update_config(
        linux::DEFAULT_AC_CONFIG_PATH,
        r#"
agents:
  nr-infra:
    agent_type: "newrelic/com.newrelic.infrastructure:0.1.0"
  nr-redis:
    agent_type: "newrelic/com.newrelic.infrastructure.nri_redis:0.1.0"
"#,
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
