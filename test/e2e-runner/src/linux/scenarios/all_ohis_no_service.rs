use crate::common::logs::expect_log_line_contains;
use crate::common::ohi::{get_all_ohi_to_test, update_infra_configs_for_ohis_without_service};
use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::common::{InstallationArgs, RecipeData};
use crate::linux;
use crate::linux::install::{install_agent_control_from_recipe, tear_down_test};
use std::time::Duration;
use tracing::info;

/// Runs a generic OHI e2e scenario for every declared OHI at once, without starting any monitored
/// service.
///
/// Installs Agent Control with the infra-agent and every OHI sub-agent side-by-side, wires each
/// one's config, and then verifies from the Agent Control logs that
/// the infra-agent invoked each integration binary with the expected config. Since no real
/// services are running, the integrations will not succeed at monitoring anything — the test only
/// checks that the binaries were triggered with the expected configs.
pub fn test_all_ohis_no_service(args: InstallationArgs) {
    let recipe_data = RecipeData {
        args: args.clone(),
        ..Default::default()
    };
    let _clean_up = CleanUp::new(tear_down_test);
    install_agent_control_from_recipe(&recipe_data);

    let ohis = get_all_ohi_to_test(&args);
    update_infra_configs_for_ohis_without_service(&ohis);

    linux::service::restart_service(linux::SERVICE_NAME);

    // Success = for every OHI there is at least one AC log line that contains BOTH
    // `integration_name=nri-<name>` AND `test.label`. Requiring them on the same line
    // proves the label was carried alongside that specific integration's invocation — i.e. the
    // config Agent Control rendered actually reached the infra-agent for that OHI.
    info!("Waiting for each OHI to be invoked with the expected label");
    retry_panic(
        60,
        Duration::from_secs(2),
        "every OHI invocation in AC log with expected label",
        || {
            for ohi in &ohis {
                let name_needle = format!("integration_name={}", ohi.name);
                expect_log_line_contains(
                    linux::DEFAULT_LOG_PATH,
                    &[name_needle.as_str(), "test.label=1.2.3"],
                )?;
            }
            Ok(())
        },
    );

    info!("all-OHIs no-service Linux scenario completed successfully");
}
