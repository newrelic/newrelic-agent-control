use crate::common::config::modify_agents_config;
use crate::common::health::wait_until_healthy;
use crate::common::logs::{clear_logs, expect_log_line_contains};
use crate::common::on_drop::CleanUp;
use crate::common::test::{retry, retry_panic};
use crate::common::{InstallationArgs, RecipeData};
use crate::windows::install::{SERVICE_NAME, install_agent_control_from_recipe, tear_down_test};
use crate::windows::scenarios::DEFAULT_STATUS_PORT;
use crate::windows::service::{
    STATUS_RUNNING, STATUS_STOPPED, check_service_status, get_scm_restart_count,
    get_service_exit_code, get_service_pid, kill_process, restart_service, stop_service,
};
use crate::windows::{self, DEFAULT_LOG_PATH};
use std::thread::sleep;
use std::time::Duration;
use tracing::{info, warn};

const SERVICE_DISPLAY_NAME: &str = "New Relic Agent Control";

/// Exercises the Windows service lifecycle end to end:
/// - a broken config makes the SCM restart the service indefinitely with no burst-limit ceiling;
/// - fixing the config recovers the service to a healthy, running state;
/// - killing the process directly makes the SCM detect it and restart the service automatically;
/// - stopping a healthy service completes gracefully, without an SCM unexpected-termination event.
pub fn test_service_lifecycle(args: InstallationArgs) {
    let recipe_data = RecipeData {
        args,
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    install_agent_control_from_recipe(&recipe_data);

    // --- Phase 1: broken config -> SCM keeps restarting without hitting a burst limit ---

    // Snapshot the current SCM restart event count before we introduce the failure.
    let baseline = get_scm_restart_count(SERVICE_DISPLAY_NAME);
    info!(baseline, "Baseline SCM restart event count");

    // Break config: unclosed brace makes YAML parsing fail immediately on every start.
    modify_agents_config(windows::DEFAULT_AC_CONFIG_PATH, "agents: {}", "agents: {");

    // The service should fail to start as the config is not right.
    restart_service(SERVICE_NAME, STATUS_STOPPED);

    // Allow 150s before giving up so CI headroom absorbs slow hosts (150s / 5s = 30 retries).
    let restarts = retry_panic(30, Duration::from_secs(5), "SCM restart count", || {
        let n = get_scm_restart_count(SERVICE_DISPLAY_NAME) - baseline;
        info!(restarts = n, "Polling SCM restart count");
        if n >= 5 {
            Ok(n)
        } else {
            Err(format!("expected at least 5 SCM restart events, got {n}").into())
        }
    });
    info!(restarts, "Reached target SCM restart count");

    // --- Phase 2: fixed config -> service recovers and reports healthy ---

    // Restore config. The SCM is already restarting every 20s; the next attempt will
    // pick up the fixed config and stay running.
    modify_agents_config(windows::DEFAULT_AC_CONFIG_PATH, "agents: {", "agents: {}");
    restart_service(SERVICE_NAME, STATUS_RUNNING);

    info!("Verifying service health");
    let status = wait_until_healthy(DEFAULT_STATUS_PORT);
    info!(response = status, "Agent Control is healthy");

    // --- Phase 3: hard kill -> the SCM detects the crash and restarts the service automatically ---

    let pid = get_service_pid(SERVICE_NAME);
    info!(pid, "Killing Agent Control process directly");
    kill_process(pid);

    retry_panic(
        10,
        Duration::from_secs(1),
        "service stopped after kill",
        || {
            check_service_status(SERVICE_NAME, STATUS_STOPPED)?;
            if get_service_exit_code(SERVICE_NAME) == 0 {
                return Err("a hard kill must not report a clean Win32 exit code of 0".into());
            }
            Ok(())
        },
    );

    let status_after_kill = wait_until_healthy(DEFAULT_STATUS_PORT);

    info!(
        response = status_after_kill,
        "Agent Control is healthy after the forced restart"
    );

    // --- Phase 4: graceful stop -> completes cleanly, not interrupted or force-killed ---

    let baseline_before_stop = get_scm_restart_count(SERVICE_DISPLAY_NAME);

    clear_logs(DEFAULT_LOG_PATH).expect("should clear logs to avoid wrong match hits");

    stop_service(SERVICE_NAME);

    // No new "service terminated unexpectedly" event: rules out a crash during shutdown.
    // Event log writes can lag the stop, so give them time to show up before asserting absence.
    sleep(Duration::from_secs(5));
    let restarts_during_stop = get_scm_restart_count(SERVICE_DISPLAY_NAME) - baseline_before_stop;
    assert_eq!(
        0, restarts_during_stop,
        "a graceful stop must not be recorded as an unexpected termination (Event 7031)"
    );

    let exit_code = get_service_exit_code(SERVICE_NAME);
    assert_eq!(
        0, exit_code,
        "a graceful stop must report a Win32 exit code of 0"
    );

    // Once Agent Control reports the service as stopped, the SCM may terminate the process at any moment, so anything
    // logged after that point (e.g. the terminal "exited successfully" line) can be lost
    let _ = retry(10, Duration::from_secs(1), "graceful exit log line", || {
        expect_log_line_contains(
            DEFAULT_LOG_PATH,
            &["The agent control main process exited successfully"],
        )
    })
    .inspect_err(|err| {
        warn!(%err, "Graceful exit log line not found, it can be lost after the SCM receives the stop")
    });

    info!(
        "Test passed: crash-loop recovery, config recovery, forced-restart recovery, and graceful shutdown all verified"
    );
}
