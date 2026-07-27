use crate::common::logs::expect_log_line_contains;
use crate::common::ohi::{
    TEST_LABEL, TEST_LABEL_VALUE, get_all_ohi_to_test,
    update_infra_configs_for_ohis_without_service,
};
use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::common::{InstallationArgs, RecipeData};
use crate::windows;
use crate::windows::install::{SERVICE_NAME, install_agent_control_from_recipe, tear_down_test};
use crate::windows::service::{STATUS_RUNNING, restart_service};
use std::time::Duration;
use tracing::info;

/// Installs Agent Control with the infra-agent and every OHI sub-agent side-by-side,
/// and then verifies from the Agent Control logs that
/// the infra-agent invoked each integration binary with a specific label. Since no real
/// services are running, the integrations will not monitor anything — the test only checks
/// that the binaries were triggered and that a specific label TEST_LABEL is passed from config.
pub fn test_all_ohis_no_service(args: InstallationArgs) {
    let recipe_data = RecipeData {
        args: args.clone(),
        ..Default::default()
    };
    let _clean_up = CleanUp::new(tear_down_test);
    install_agent_control_from_recipe(&recipe_data);

    let ohis = get_all_ohi_to_test(&args);
    update_infra_configs_for_ohis_without_service(&ohis);

    restart_service(SERVICE_NAME, STATUS_RUNNING);

    info!(
        "Waiting for each OHI to be invoked with the expected label, notice that debug level is needed in order to assert such logs"
    );
    retry_panic(
        60,
        Duration::from_secs(2),
        "every OHI invocation in AC log with expected label",
        || {
            for ohi in &ohis {
                expect_log_line_contains(
                    windows::DEFAULT_LOG_PATH,
                    &[
                        "component=integrations.runner.Runner",
                        &format!("integration_name={}", ohi.name),
                        &format!("{TEST_LABEL}={TEST_LABEL_VALUE}"),
                    ],
                )?;
            }
            Ok(())
        },
    );

    info!("all-OHIs no-service Windows scenario completed successfully");
}
