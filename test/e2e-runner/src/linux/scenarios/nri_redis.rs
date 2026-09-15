use crate::common::config::{DEBUG_LOGGING_CONFIG, update_config, write_agent_local_config};
use crate::common::logs::show_logs;
use crate::common::nrql::{self, Region};
use crate::common::ohi::{
    EMBEDDED_LINUX_OHI_BINARIES, EMBEDDED_LINUX_OHI_CONFIGS, SHARED_LINUX_FILESYSTEM_DIR,
    check_ohi_shared_filesystem,
};
use crate::common::on_drop::CleanUp;
use crate::common::runtime::tokio_runtime;
use crate::common::test::retry_panic;
use crate::common::{InstallationArgs, RecipeData};
use crate::linux;
use crate::linux::install::{install_agent_control_from_recipe, tear_down_test};
use crate::linux::redis::Redis;
use crate::linux::service::{STATUS_RUNNING, restart_service_and_wait};
use fake_opamp_server::FakeServer;
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

    // Start the fake OpAMP server first — its endpoint is needed in AC's master config below.
    let mut opamp_server = FakeServer::start(tokio_runtime().handle());
    info!(
        endpoint = opamp_server.endpoint(),
        "Fake OpAMP server started"
    );

    let _clean_up = CleanUp::new(tear_down_test);
    // Drops in reverse declaration order — this runs before `_clean_up` so the infra-agent
    // log lands next to (not after) the AC logs on failure or success.
    let _infra_log_dump = CleanUp::new(|| {
        let _ = show_logs(INFRA_AGENT_LOG_PATH);
    });

    install_agent_control_from_recipe(&recipe_data);

    let _redis = Redis::start();

    // Both per-agent local configs are written up front — AC needs them present the moment
    // a remote config lists these agents.
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

    // Master AC config: no agents initially — both will come via OpAMP push. Point fleet_control
    // at the local fake server so we can add/remove agents without any mid-test restart.
    let master_config = format!(
        r#"
host_id: {test_id}
agents: {{}}
fleet_control:
  endpoint: {opamp_endpoint}
  poll_interval: 5s
  signature_validation:
    public_key_server_url: {jwks_endpoint}
{DEBUG_LOGGING_CONFIG}
"#,
        opamp_endpoint = opamp_server.endpoint(),
        jwks_endpoint = opamp_server.jwks_endpoint(),
    );
    update_config(linux::DEFAULT_AC_CONFIG_PATH, &master_config);

    restart_service_and_wait(linux::SERVICE_NAME, STATUS_RUNNING);

    // Wait for AC to connect to the fake OpAMP server.
    let ac_instance_id = retry_panic(
        20,
        Duration::from_secs(2),
        "AC connecting to OpAMP server",
        || {
            opamp_server
                .find_agent_control_instance()
                .map_err(|e| e.into())
        },
    );
    info!("AC connected to fake OpAMP server");

    // Phase 1: push nr-infra via OpAMP; AC applies it against a running instance and starts
    // newrelic-infra. Wait for SystemSample so phase 2 truly targets a steady-state AC.
    info!("Phase 1: pushing nr-infra via OpAMP");
    opamp_server.set_config_response(
        ac_instance_id.clone(),
        r#"
agents:
  nr-infra:
    agent_type: "newrelic/com.newrelic.infrastructure:0.1.0"
"#,
    );

    let system_sample_query =
        format!(r#"SELECT * FROM SystemSample WHERE `host.id` = '{test_id}' LIMIT 1"#);
    info!(
        nrql = system_sample_query,
        "Waiting for SystemSample data in NRDB (nr-infra steady state)"
    );
    retry_panic(60, Duration::from_secs(10), "SystemSample NRQL", || {
        nrql::check_query_results_are_not_empty(&recipe_data.args, &system_sample_query)
    });

    // Phase 2: push nr-infra + nr-redis via OpAMP. No service restart — AC picks it up on its
    // next poll (≤5s) and spins up nr-redis alongside the already-running nr-infra. This is the
    // "add OHI to a running system" path where the config-lands-before-binary race has its
    // widest window.
    info!("Phase 2: pushing nr-redis via OpAMP (no restart)");
    opamp_server.set_config_response(
        ac_instance_id.clone(),
        r#"
agents:
  nr-infra:
    agent_type: "newrelic/com.newrelic.infrastructure:0.1.0"
  nr-redis:
    agent_type: "newrelic/com.newrelic.infrastructure.nri_redis:0.1.0"
"#,
    );

    let nrql_query =
        format!(r#"SELECT * FROM RedisSample WHERE `label.test.id` = '{test_id}' LIMIT 1"#);
    info!(nrql = nrql_query, "Waiting for RedisSample data in NRDB");
    retry_panic(10, Duration::from_secs(10), "RedisSample NRQL", || {
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
