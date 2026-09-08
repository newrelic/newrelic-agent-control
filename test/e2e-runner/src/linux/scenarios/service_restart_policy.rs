use crate::common::config::modify_agents_config;
use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::common::{InstallationArgs, RecipeData};
use crate::linux;
use crate::linux::install::{install_agent_control_from_recipe, tear_down_test};
use crate::linux::service::{
    STATUS_RUNNING, get_auto_restart_count, get_service_status, is_start_limit_hit, restart_service,
};
use std::time::Duration;
use tracing::info;

/// Verifies that the service restarts indefinitely on failure with no burst-limit ceiling.
///
/// The test breaks the YAML config so AC exits immediately on every start, then waits long
/// enough for at least 5 automatic restarts (≥5 × 20 s delay).
pub fn test_service_restarts_indefinitely_on_failure(args: InstallationArgs) {
    let recipe_data = RecipeData {
        args,
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    install_agent_control_from_recipe(&recipe_data);

    // Break config: unclosed brace makes YAML parsing fail immediately on every start.
    modify_agents_config(linux::DEFAULT_AC_CONFIG_PATH, "agents: {}", "agents: {");

    // With StartLimitIntervalSec=0 and Restart=always, the service never enters "failed"
    // or "active" state — ActiveState cycles between "activating" sub-states indefinitely.
    // Just trigger the restart; NRestarts accumulates as systemd auto-retries.
    restart_service(linux::SERVICE_NAME);

    // Poll until we see at least 5 auto-restarts (20s RestartSec → ≥100s minimum).
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
    info!("Test passed: service restarted indefinitely without hitting the burst limit");
}
