use std::path::Path;

use crate::common::file::remove_dirs;
use crate::common::logs::show_logs;
use crate::common::on_drop::CleanUp;
use crate::common::{InstallationArgs, RecipeData};
use crate::windows::install::install_agent_control_from_recipe;
use crate::windows::powershell::exec_ps;
use crate::windows::service::{STATUS_RUNNING, check_service_status};
use crate::windows::{AGENT_CONTROL_DIRS, DEFAULT_LOG_PATH};
use tracing::{info, warn};

const UNINSTALL_SCRIPT: &str = r"C:\Program Files\New Relic\newrelic-agent-control\uninstall.ps1";
const SERVICE_NAME: &str = "newrelic-agent-control";
const INSTALL_DIR: &str = r"C:\Program Files\New Relic\newrelic-agent-control";
const RUNTIME_DIR: &str = r"C:\ProgramData\New Relic\newrelic-agent-control";

pub fn test_uninstall_locked_directory(args: InstallationArgs) {
    let recipe_data = RecipeData {
        args,
        ..Default::default()
    };

    let _clean_up = CleanUp::new(|| {
        let _ = show_logs(DEFAULT_LOG_PATH).inspect_err(|e| warn!("Fail to show logs: {}", e));
        let _ = remove_dirs(AGENT_CONTROL_DIRS)
            .inspect_err(|err| warn!("Failed to remove Agent Control directories: {}", err));
    });

    install_agent_control_from_recipe(&recipe_data);

    // Simulate Explorer holding the folder open: open uninstall.ps1 without FILE_SHARE_DELETE so
    // Remove-Item cannot delete it (ERROR_SHARING_VIOLATION). Rust's default File::open includes
    // FILE_SHARE_DELETE, which lets deletions succeed even with an open handle, we must
    // explicitly exclude it via OpenOptionsExt.
    let lock_path = Path::new(INSTALL_DIR).join("uninstall.ps1");
    #[cfg(target_os = "windows")]
    let _lock = {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 0x1;
        const FILE_SHARE_WRITE: u32 = 0x2;
        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(&lock_path)
            .unwrap_or_else(|e| panic!("failed to open lock file {}: {}", lock_path.display(), e))
    };
    #[cfg(not(target_os = "windows"))]
    let _lock = std::fs::File::open(&lock_path)
        .unwrap_or_else(|e| panic!("failed to open lock file {}: {}", lock_path.display(), e));

    info!("Running uninstall script with install directory locked");
    let result = exec_ps(format!("& '{UNINSTALL_SCRIPT}'"));

    info!("Asserting script failed with a descriptive error for the locked directory");
    let err = result
        .expect_err("uninstall script should exit non-zero when a directory cannot be removed")
        .to_string();
    assert!(
        err.contains("Could not remove"),
        "expected error message about locked directory, got:\n{err}"
    );

    info!("Asserting install directory still exists after failed removal");
    exec_ps(format!(
        "if (Test-Path '{INSTALL_DIR}') {{ exit 0 }} else {{ exit 1 }}"
    ))
    .expect("install directory should still exist while the file handle is held");

    info!("Asserting service was still removed despite the locked directory");
    exec_ps(format!(
        "if (Get-Service -Name '{SERVICE_NAME}' -ErrorAction SilentlyContinue) {{ exit 1 }} else {{ exit 0 }}"
    ))
    .expect("service should not be present even when directory removal failed");

    // Dropping the lock here; CleanUp above handles the remaining directory.
    drop(_lock);
    info!("Locked-directory uninstall assertions passed");
}

pub fn test_uninstall_and_reinstall(args: InstallationArgs) {
    let recipe_data = RecipeData {
        args,
        ..Default::default()
    };

    let _clean_up = CleanUp::new(|| {
        let _ = show_logs(DEFAULT_LOG_PATH).inspect_err(|e| warn!("Fail to show logs: {}", e));
        let _ = remove_dirs(AGENT_CONTROL_DIRS)
            .inspect_err(|err| warn!("Failed to remove Agent Control directories: {}", err));
    });

    info!("Installing Agent Control (first install)");
    install_agent_control_from_recipe(&recipe_data);

    info!("Running uninstall script at {UNINSTALL_SCRIPT}");
    exec_ps(format!("& '{UNINSTALL_SCRIPT}'")).expect("uninstall script should exit successfully");

    info!("Asserting clean removal before reinstall");
    exec_ps(format!(
        "if (Get-Service -Name '{SERVICE_NAME}' -ErrorAction SilentlyContinue) {{ exit 1 }} else {{ exit 0 }}"
    ))
    .expect("service should not be present after uninstall");
    exec_ps(format!(
        "if (Test-Path '{INSTALL_DIR}') {{ exit 1 }} else {{ exit 0 }}"
    ))
    .expect("install directory should be removed after uninstall");
    exec_ps(format!(
        "if (Test-Path '{RUNTIME_DIR}') {{ exit 1 }} else {{ exit 0 }}"
    ))
    .expect("runtime data directory should be removed after uninstall");

    info!("Installing Agent Control (reinstall)");
    install_agent_control_from_recipe(&recipe_data);

    info!("Asserting service is running after reinstall");
    check_service_status(SERVICE_NAME, STATUS_RUNNING)
        .expect("service should be running after reinstall");

    info!("Asserting install directory is present after reinstall");
    exec_ps(format!(
        "if (Test-Path '{INSTALL_DIR}') {{ exit 0 }} else {{ exit 1 }}"
    ))
    .expect("install directory should be present after reinstall");

    info!("Uninstall-and-reinstall assertions passed");
}
