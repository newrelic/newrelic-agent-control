use crate::common::on_drop::CleanUp;
use crate::common::{InstallationArgs, RecipeData};
use crate::linux::SERVICE_NAME;
use crate::linux::bash::exec_bash_command;
use crate::linux::install::{install_agent_control_from_recipe, tear_down_test};
use crate::linux::service::{ENABLED, STATUS_RUNNING, get_service_status, get_unit_file_state};
use tracing::info;

const UNINSTALL_SCRIPT: &str = "/usr/lib/newrelic-agent-control/uninstall.sh";
const BINARY_PATH: &str = "/usr/bin/newrelic-agent-control";
const STATIC_CONFIG_DIR: &str = "/etc/newrelic-agent-control";
const RUNTIME_DATA_DIR: &str = "/var/lib/newrelic-agent-control";
// Only created when file logging is explicitly enabled (log.file.enabled: true for AC itself,
// or enable_file_logging: true per sub-agent). Not created by the package installer or by default.
const LOG_DIR: &str = "/var/log/newrelic-agent-control";

pub fn test_uninstall_and_reinstall(args: InstallationArgs) {
    let recipe_data = RecipeData {
        args,
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    info!("Installing Agent Control (first install)");
    install_agent_control_from_recipe(&recipe_data);

    // File logging is not enabled by default; seed the directory so we can assert it is cleaned up.
    exec_bash_command(&format!("mkdir -p '{LOG_DIR}'"))
        .expect("should be able to create log directory");

    info!("Running uninstall script at {UNINSTALL_SCRIPT}");
    exec_bash_command(&format!("bash {UNINSTALL_SCRIPT}"))
        .expect("uninstall script should exit successfully");

    info!("Asserting clean removal before reinstall");
    let service_present = exec_bash_command("systemctl cat newrelic-agent-control 2>/dev/null");
    assert!(
        service_present.is_err(),
        "service unit file should be absent after uninstall"
    );
    exec_bash_command(&format!("test ! -f '{BINARY_PATH}'"))
        .expect("binary should be removed after uninstall");
    exec_bash_command(&format!("test ! -d '{STATIC_CONFIG_DIR}'"))
        .expect("static config directory should be removed after uninstall");
    exec_bash_command(&format!("test ! -d '{RUNTIME_DATA_DIR}'"))
        .expect("runtime data directory should be removed after uninstall");
    exec_bash_command(&format!("test ! -d '{LOG_DIR}'"))
        .expect("log directory should be removed after uninstall");

    info!("Installing Agent Control (reinstall)");
    install_agent_control_from_recipe(&recipe_data);

    info!("Asserting service is running and enabled after reinstall");
    assert_eq!(
        get_unit_file_state(SERVICE_NAME),
        ENABLED,
        "service should be enabled after reinstall"
    );
    assert_eq!(
        get_service_status(SERVICE_NAME),
        STATUS_RUNNING,
        "service should be running after reinstall \
         (regression: missing dpkg conffile caused service startup failure)"
    );

    info!("Asserting binary and config are present after reinstall");
    exec_bash_command(&format!("test -f '{BINARY_PATH}'"))
        .expect("binary should be present after reinstall");
    exec_bash_command(&format!("test -d '{STATIC_CONFIG_DIR}'"))
        .expect("static config directory should be present after reinstall");

    info!("Uninstall-and-reinstall assertions passed");
}
