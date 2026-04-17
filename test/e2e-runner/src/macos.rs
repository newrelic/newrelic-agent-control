use crate::common::fleet_control_api;
use crate::{MacOSCli, MacOSScenarios, init_logging};
use clap::Parser;

/// Run macOS e2e corresponding scenario which will panic on failure
pub fn run_macos_e2e() {
    let cli = MacOSCli::parse();
    init_logging(&cli.log_level);

    // Run the requested test
    match cli.scenario {
        MacOSScenarios::FleetControlApi(args) => {
            fleet_control_api::run_fleet_control_api(args);
        }
    };
}
