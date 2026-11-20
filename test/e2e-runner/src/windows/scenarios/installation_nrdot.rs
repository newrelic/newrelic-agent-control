use crate::common::agent_metadata::{AgentKind, AgentMetadata};
use crate::common::config::nrdot_config;
use crate::common::config::{DEBUG_LOGGING_CONFIG, update_config, write_agent_local_config};
use crate::common::health::check_health;
use crate::common::logs::{expect_file_line_contains, expect_log_line_contains, show_log_tail};
use crate::common::on_drop::CleanUp;
use crate::common::test::{retry, retry_panic};
use crate::common::{InstallationArgs, RecipeData, nrql};
use crate::windows::install::{SERVICE_NAME, install_agent_control_from_recipe, tear_down_test};
use crate::windows::powershell::test_file_exists;
use crate::windows::scenarios::DEFAULT_STATUS_PORT;
use crate::windows::service::{STATUS_RUNNING, stop_service};
use crate::windows::{self, DEFAULT_LOG_PATH};
use std::thread;
use std::time::Duration;
use tracing::info;

/// Runs a complete Windows E2E installation test with NRDOT
pub fn test_nrdot(args: InstallationArgs) {
    let AgentMetadata {
        sub_agent_id: nrdot_sub_agent_id,
        agent_type_id: nrdot_agent_type,
        package_version: nrdot_version,
    } = args.agent(AgentKind::Nrdot);
    let recipe_data = RecipeData {
        args,
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    install_agent_control_from_recipe(&recipe_data);

    let test_id = format!(
        "onhost-e2e-nrdot_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S%.3f")
    );

    update_config(
        windows::DEFAULT_AC_CONFIG_PATH,
        format!(
            r#"
host_id: {test_id}
agents:
  {nrdot_sub_agent_id}:
    agent_type: {nrdot_agent_type}
{DEBUG_LOGGING_CONFIG}
"#
        ),
    );

    write_agent_local_config(
        &windows::local_config_path(nrdot_sub_agent_id),
        nrdot_config(&nrdot_version),
    );

    windows::service::restart_service(SERVICE_NAME, STATUS_RUNNING);
    info!("Waiting 10 seconds for service to start");
    thread::sleep(Duration::from_secs(10));

    windows::service::check_service_status(SERVICE_NAME, STATUS_RUNNING)
        .expect("service should be running");

    info!("Verifying service health");
    let status_endpoint = format!("http://localhost:{DEFAULT_STATUS_PORT}/status");
    let status = retry(30, Duration::from_secs(2), "health check", || {
        check_health(&status_endpoint)
    })
    .unwrap_or_else(|err| panic!("Health check failed: {err}"));

    info!("Agent Control is healthy");
    let status_json = serde_json::to_string_pretty(&status)
        .unwrap_or_else(|err| panic!("Failed to serialize status to JSON: {err}"));
    info!(response = status_json, "Agent Control is healthy");

    let log_file = windows::sub_agent_log_path(nrdot_sub_agent_id, "nrdot-collector.log");
    info!(path = log_file, "Checking the nr-dot log file is created");
    retry_panic(30, Duration::from_secs(2), "nr-dot log file", || {
        test_file_exists(&log_file)
    });

    let nrql_query = format!(
        r#"SELECT `system.memory.utilization` FROM Metric WHERE `host.id` = '{test_id}' LIMIT 1"#
    );
    info!(nrql = nrql_query, "Checking results of NRQL");
    let retries = 60;
    retry_panic(retries, Duration::from_secs(10), "nrql assertion", || {
        nrql::check_query_results_are_not_empty(&recipe_data.args, &nrql_query)
    });

    // Stopping the service stops the collector: Agent Control sends it CTRL_BREAK_EVENT, which Go
    // delivers as os.Interrupt, and the collector shuts down when it receives it.
    info!("Stopping Agent Control to check the graceful shutdown of the collector");
    stop_service(SERVICE_NAME);
    show_log_tail(&log_file, 40);

    retry_panic(
        10,
        Duration::from_secs(1),
        "collector received the signal",
        || expect_file_line_contains(&log_file, &["Received signal from OS"]),
    );
    retry_panic(
        10,
        Duration::from_secs(1),
        "collector exited after the graceful shutdown signal",
        || {
            expect_log_line_contains(
                DEFAULT_LOG_PATH,
                &[
                    "Process exited after the graceful shutdown signal",
                    nrdot_sub_agent_id,
                ],
            )
        },
    );
    // 0xc000013a is the exit status of the OS default handler, i.e. the collector did not handle
    // the signal itself.
    assert!(
        expect_log_line_contains(
            DEFAULT_LOG_PATH,
            &[
                "Process exited after the graceful shutdown signal",
                nrdot_sub_agent_id,
                "0xc000013a",
            ],
        )
        .is_err(),
        "the collector should handle CTRL_BREAK_EVENT, not be ended by the OS default handler"
    );

    info!("Test completed successfully");
}
