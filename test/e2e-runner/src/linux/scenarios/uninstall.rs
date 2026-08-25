use crate::common::on_drop::CleanUp;
use crate::common::{InstallationArgs, RecipeData};
use crate::linux::bash::exec_bash_command;
use crate::linux::install::{install_agent_control_from_recipe, tear_down_test};
use tracing::info;

const UNINSTALL_SCRIPT: &str = "/usr/lib/newrelic-agent-control/uninstall.sh";
const BINARY_PATH: &str = "/usr/bin/newrelic-agent-control";
const STATIC_CONFIG_DIR: &str = "/etc/newrelic-agent-control";
const RUNTIME_DATA_DIR: &str = "/var/lib/newrelic-agent-control";

pub fn test_uninstall_script(args: InstallationArgs) {
    let recipe_data = RecipeData {
        args,
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    install_agent_control_from_recipe(&recipe_data);

    info!("Running uninstall script at {UNINSTALL_SCRIPT}");
    exec_bash_command(&format!("bash {UNINSTALL_SCRIPT}"))
        .expect("uninstall script should exit successfully");

    info!("Asserting service unit is gone");
    // systemctl cat exits non-zero when the unit file has been removed by the package manager.
    let service_present = exec_bash_command("systemctl cat newrelic-agent-control 2>/dev/null");
    assert!(
        service_present.is_err(),
        "service unit file should be absent after uninstall"
    );

    info!("Asserting binary is removed");
    exec_bash_command(&format!("test ! -f '{BINARY_PATH}'"))
        .expect("binary should be removed after uninstall");

    info!("Asserting static config directory is removed");
    exec_bash_command(&format!("test ! -d '{STATIC_CONFIG_DIR}'"))
        .expect("static config directory should be removed after uninstall");

    info!("Asserting runtime data directory is removed");
    exec_bash_command(&format!("test ! -d '{RUNTIME_DATA_DIR}'"))
        .expect("runtime data directory should be removed after uninstall");

    info!("Uninstall assertions passed");
}
