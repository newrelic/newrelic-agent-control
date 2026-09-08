use crate::common::config::modify_agents_config;
use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::common::{InstallationArgs, RecipeData};
use crate::windows;
use crate::windows::install::{SERVICE_NAME, install_agent_control_from_recipe, tear_down_test};
use crate::windows::service::{
    STATUS_RUNNING, check_service_status, get_scm_restart_count, restart_service,
};
use std::time::Duration;
use tracing::info;

const SERVICE_DISPLAY_NAME: &str = "New Relic Agent Control";

/// Verifies that the service restarts indefinitely on failure with no burst-limit ceiling.
///
/// The test breaks the YAML config so AC exits immediately on every start, records the
/// baseline SCM restart event count, then waits long enough for at least 5 automatic
/// restarts (≥5 × 20 s delay). It asserts the delta of SCM Event 7031 entries (service
/// terminated unexpectedly, recovery action taken) is ≥5, confirming the SCM kept
/// restarting instead of giving up. Finally it restores the config and verifies recovery.
pub fn test_service_restarts_indefinitely_on_failure(args: InstallationArgs) {
    let recipe_data = RecipeData {
        args,
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    install_agent_control_from_recipe(&recipe_data);

    // Snapshot the current SCM restart event count before we introduce the failure.
    let baseline = get_scm_restart_count(SERVICE_DISPLAY_NAME);
    info!(baseline, "Baseline SCM restart event count");

    // Break config: unclosed brace makes YAML parsing fail immediately on every start.
    modify_agents_config(windows::DEFAULT_AC_CONFIG_PATH, "agents: {}", "agents: {");

    restart_service(SERVICE_NAME, windows::service::STATUS_STOPPED);

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

    // Restore config. The SCM is already restarting every 20s; the next attempt will
    // pick up the fixed config and stay running.
    modify_agents_config(windows::DEFAULT_AC_CONFIG_PATH, "agents: {", "agents: {}");
    retry_panic(
        15,
        Duration::from_secs(5),
        "service recovery after config fix",
        || check_service_status(SERVICE_NAME, STATUS_RUNNING),
    );

    info!("Test passed: service restarted indefinitely without hitting a burst limit");
}
