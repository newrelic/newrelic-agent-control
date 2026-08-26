use crate::common::file::remove_dirs;
use crate::common::logs::show_logs;
use crate::common::on_drop::CleanUp;
use crate::common::{InstallationArgs, RecipeData};
use crate::windows::install::install_agent_control_from_recipe;
use crate::windows::powershell::exec_ps;
use crate::windows::{AGENT_CONTROL_DIRS, DEFAULT_LOG_PATH};
use tracing::{info, warn};

const UNINSTALL_SCRIPT: &str = r"C:\Program Files\New Relic\newrelic-agent-control\uninstall.ps1";
const SERVICE_NAME: &str = "newrelic-agent-control";
const INSTALL_DIR: &str = r"C:\Program Files\New Relic\newrelic-agent-control";
const RUNTIME_DIR: &str = r"C:\ProgramData\New Relic\newrelic-agent-control";

pub fn test_uninstall_script(args: InstallationArgs) {
    let recipe_data = RecipeData {
        args,
        ..Default::default()
    };

    // Custom teardown: the uninstall script removes the service, so we must not call
    // stop_service() here — it would panic on "service not found".
    let _clean_up = CleanUp::new(|| {
        let _ = show_logs(DEFAULT_LOG_PATH).inspect_err(|e| warn!("Fail to show logs: {}", e));
        let _ = remove_dirs(AGENT_CONTROL_DIRS)
            .inspect_err(|err| warn!("Failed to remove Agent Control directories: {}", err));
    });

    install_agent_control_from_recipe(&recipe_data);

    info!("Running uninstall script at {UNINSTALL_SCRIPT}");
    exec_ps(format!("& '{UNINSTALL_SCRIPT}'")).expect("uninstall script should exit successfully");

    info!("Asserting service is gone");
    // Exit 1 if the service still exists, 0 if absent.
    exec_ps(format!(
        "if (Get-Service -Name '{SERVICE_NAME}' -ErrorAction SilentlyContinue) {{ exit 1 }} else {{ exit 0 }}"
    ))
    .expect("service should not be present after uninstall");

    info!("Asserting install directory is removed");
    // exit 1 if the path still exists, 0 if absent
    exec_ps(format!(
        "if (Test-Path '{INSTALL_DIR}') {{ exit 1 }} else {{ exit 0 }}"
    ))
    .expect("install directory should be removed after uninstall");

    info!("Asserting runtime data directory is removed");
    exec_ps(format!(
        "if (Test-Path '{RUNTIME_DIR}') {{ exit 1 }} else {{ exit 0 }}"
    ))
    .expect("runtime data directory should be removed after uninstall");

    info!("Uninstall assertions passed");
}
