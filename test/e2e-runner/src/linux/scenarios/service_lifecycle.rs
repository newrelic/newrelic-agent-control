use crate::common::config::{modify_agents_config, update_config};
use crate::common::health::wait_until_healthy;
use crate::common::logs::{clear_logs, expect_log_line_contains};
use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::common::{InstallationArgs, RecipeData};
use crate::linux::install::{install_agent_control_from_recipe, tear_down_test};
use crate::linux::scenarios::DEFAULT_STATUS_PORT;
use crate::linux::service::{
    STATUS_RUNNING, get_auto_restart_count, get_main_pid, get_service_result, get_service_status,
    is_start_limit_hit, kill_process, restart_service, stop_service,
};
use crate::linux::{self, DEFAULT_LOG_PATH};
use std::time::Duration;
use tracing::info;

const FILE_LOGGING_CONFIG: &str = r#"
log:
  file:
    enabled: true
"#;

/// Exercises the systemd service lifecycle end to end:
/// - a broken config makes systemd restart the service indefinitely with no burst-limit ceiling;
/// - fixing the config recovers the service to a healthy, running state;
/// - killing the process directly makes systemd detect it and restart the service automatically;
/// - stopping a healthy service completes as a clean exit, not a timeout/signal/kill, and logs the
///   terminal success line.
pub fn test_service_lifecycle(args: InstallationArgs) {
    let recipe_data = RecipeData {
        args,
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    install_agent_control_from_recipe(&recipe_data);

    // File logging is off by default. It is required to assert on the terminal log line in Phase 4.
    update_config(linux::DEFAULT_AC_CONFIG_PATH, FILE_LOGGING_CONFIG);

    // --- Phase 1: broken config -> systemd keeps restarting without hitting the burst limit ---

    // Break config: unclosed brace makes YAML parsing fail immediately on every start.
    modify_agents_config(linux::DEFAULT_AC_CONFIG_PATH, "agents: {}", "agents: {");

    restart_service(linux::SERVICE_NAME);

    // Poll until we see at least 5 auto-restarts (20s RestartSec -> >=100s minimum).
    // Allow 180s before giving up so CI headroom absorbs slow hosts (180s / 5s = 36 retries).
    let n_restarts = retry_panic(36, Duration::from_secs(5), "auto-restart count", || {
        let n = get_auto_restart_count(linux::SERVICE_NAME);
        info!(n_restarts = n, "Polling restart count");
        if n >= 5 {
            Ok(n)
        } else {
            Err(format!("expected at least 5 auto-restarts, got {n}").into())
        }
    });
    info!(n_restarts, "Reached target restart count");

    assert!(
        !is_start_limit_hit(linux::SERVICE_NAME),
        "StartLimitHit must be false"
    );

    // --- Phase 2: fixed config -> service recovers and reports healthy ---

    // Restore config. systemd is already restarting every 20s; the next attempt will
    // pick up the fixed config and stay running.
    modify_agents_config(linux::DEFAULT_AC_CONFIG_PATH, "agents: {", "agents: {}");
    retry_panic(
        15,
        Duration::from_secs(5),
        "service recovery after config fix",
        || {
            let status = get_service_status(linux::SERVICE_NAME);
            if status == STATUS_RUNNING {
                Ok(())
            } else {
                Err(format!("service not yet active, current status: {status}").into())
            }
        },
    );

    info!("Verifying service health");
    let status = wait_until_healthy(DEFAULT_STATUS_PORT);
    info!(response = status, "Agent Control is healthy");

    // --- Phase 3: hard kill -> systemd detects the crash and restarts automatically ---

    let pid = get_main_pid(linux::SERVICE_NAME);
    info!(pid, "Killing Agent Control process directly");
    kill_process(pid);

    retry_panic(
        15,
        Duration::from_secs(1),
        "service not running after kill",
        || {
            let status = get_service_status(linux::SERVICE_NAME);
            if status == STATUS_RUNNING {
                return Err(format!("service still reports {status} right after the kill").into());
            }
            let result = get_service_result(linux::SERVICE_NAME);
            if result != "signal" {
                return Err(format!(
                    "a hard kill must be reported as killed by a signal, not a clean exit, got {result}"
                )
                .into());
            }
            Ok(())
        },
    );

    let status_after_kill = wait_until_healthy(DEFAULT_STATUS_PORT);

    info!(
        response = status_after_kill,
        "Agent Control is healthy after the forced restart"
    );

    // --- Phase 4: graceful stop -> completes as a clean exit, not a timeout/signal/kill ---

    clear_logs(DEFAULT_LOG_PATH).expect("should clear logs to avoid wrong match hits");

    stop_service(linux::SERVICE_NAME);
    assert_eq!(
        get_service_result(linux::SERVICE_NAME),
        "success",
        "a graceful stop must be reported as a clean exit, not a timeout/signal/kill"
    );

    retry_panic(10, Duration::from_secs(1), "graceful exit log line", || {
        expect_log_line_contains(
            DEFAULT_LOG_PATH,
            &["The agent control main process exited successfully"],
        )
    });

    info!(
        "Test passed: crash-loop recovery, config recovery, forced-restart recovery, and graceful shutdown all verified"
    );
}
