use crate::common::agent_metadata::AgentKind;
use crate::common::config::{DEBUG_LOGGING_CONFIG, update_config, write_agent_local_config};
use crate::common::nrql;
use crate::common::ohi::{
    EMBEDDED_WINDOWS_OHI_BINARIES, SHARED_WINDOWS_FILESYSTEM_DIR, check_ohi_shared_filesystem,
};
use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::common::{InstallationArgs, RecipeData};
use crate::windows;
use crate::windows::install::{SERVICE_NAME, install_agent_control_from_recipe, tear_down_test};
use crate::windows::redis::Redis;
use crate::windows::service::{STATUS_RUNNING, restart_service};
use std::time::Duration;
use tracing::info;

pub fn test_nri_redis(args: InstallationArgs) {
    let infra_sub_agent_id = AgentKind::InfraAgent.sub_agent_id();
    let redis_sub_agent_id = AgentKind::Redis.sub_agent_id();
    let redis_agent_type = args.agent_metadata(AgentKind::Redis).agent_type_id();
    let infra_agent_type = args.agent_metadata(AgentKind::InfraAgent).agent_type_id();
    let infra_agent_version = args
        .agent_metadata(AgentKind::InfraAgent)
        .package_version()
        .to_string();

    let redis_version = args
        .agent_metadata(AgentKind::Redis)
        .package_version()
        .to_string();

    let test_id = format!(
        "onhost-e2e-nri-redis_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S%.3f"),
    );

    let recipe_data = RecipeData {
        args: args.clone(),
        ..Default::default()
    };
    let _clean_up = CleanUp::new(tear_down_test);
    install_agent_control_from_recipe(&recipe_data);

    let _redis = Redis::start();

    update_config(
        windows::DEFAULT_AC_CONFIG_PATH,
        format!(
            r#"
host_id: {test_id}
agents:
  {infra_sub_agent_id}:
    agent_type: "{infra_agent_type}"
  {redis_sub_agent_id}:
    agent_type: "{redis_agent_type}"
{DEBUG_LOGGING_CONFIG}
"#
        ),
    );

    write_agent_local_config(
        &windows::local_config_path(infra_sub_agent_id),
        format!(
            r#"
config_agent:
  license_key: '{{{{NEW_RELIC_LICENSE_KEY}}}}'
  log:
    level: debug
version: {infra_agent_version}
"#
        ),
    );

    write_agent_local_config(
        &windows::local_config_path(redis_sub_agent_id),
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

    restart_service(SERVICE_NAME, STATUS_RUNNING);

    let nrql_query =
        format!(r#"SELECT * FROM RedisSample WHERE `label.test.id` = '{test_id}' LIMIT 1"#);
    info!(nrql = nrql_query, "Waiting for RedisSample data in NRDB");
    retry_panic(60, Duration::from_secs(10), "RedisSample NRQL", || {
        nrql::check_query_results_are_not_empty(&recipe_data.args, &nrql_query)
    });

    info!("Verifying shared-filesystem files were populated by AC");
    let expected_binaries = [EMBEDDED_WINDOWS_OHI_BINARIES.as_slice(), &["nri-redis.exe"]].concat();
    let expected_configs = ["nri-redis.yaml"].as_slice();

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

    info!("nri-redis Windows scenario completed successfully");
}
