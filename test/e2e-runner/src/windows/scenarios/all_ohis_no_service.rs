use crate::common::logs::expect_log_line_contains;
use crate::common::ohi::{get_all_ohi_to_test, update_infra_configs_for_ohis_without_service};
use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::common::{InstallationArgs, RecipeData};
use crate::windows;
use crate::windows::install::{SERVICE_NAME, install_agent_control_from_recipe, tear_down_test};
use crate::windows::service::{STATUS_RUNNING, restart_service};
use std::time::Duration;
use tracing::info;

/// Runs a generic OHI e2e scenario for every declared OHI at once, without starting any monitored
/// service. See the Linux counterpart for the shape of the assertions.
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

    info!("Waiting for each OHI to be invoked with the expected label");
    retry_panic(
        60,
        Duration::from_secs(2),
        "every OHI invocation in AC log with expected label",
        || {
            for ohi in &ohis {
                let name_needle = format!("integration_name={}", ohi.name);
                expect_log_line_contains(
                    windows::DEFAULT_LOG_PATH,
                    &[name_needle.as_str(), "test.label=1.2.3"],
                )?;
            }
            Ok(())
        },
    );

    info!("all-OHIs no-service Windows scenario completed successfully");
}
