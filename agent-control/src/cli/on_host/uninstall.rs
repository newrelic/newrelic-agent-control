use crate::cli::common::error::CliError;
use clap::Args;
use std::io::Write as _;
use std::path::Path;
use std::process::Command;
use std::time::Duration;
#[cfg(target_family = "unix")]
use std::time::Instant;
use tracing::{info, warn};

#[cfg(target_family = "unix")]
const AC_BINARY: &str = "/usr/bin/newrelic-agent-control";
#[cfg(target_family = "unix")]
const AC_CLI_BINARY: &str = "/usr/bin/newrelic-agent-control-cli";
#[cfg(target_family = "windows")]
const AC_BINARY: &str =
    r"C:\Program Files\New Relic\newrelic-agent-control\newrelic-agent-control.exe";
#[cfg(target_family = "windows")]
const AC_CLI_BINARY: &str =
    r"C:\Program Files\New Relic\newrelic-agent-control\newrelic-agent-control-cli.exe";

const LOCAL_DATA_DIR: &str = "/etc/newrelic-agent-control";
const REMOTE_DATA_DIR: &str = "/var/lib/newrelic-agent-control";
const AC_SERVICE_NAME: &str = "newrelic-agent-control";

#[cfg(target_family = "unix")]
const SYSTEMD_UNIT: &str = "/etc/systemd/system/newrelic-agent-control.service";

/// Maximum time to wait for the service to stop before proceeding.
#[cfg(target_family = "unix")]
const SERVICE_STOP_TIMEOUT: Duration = Duration::from_secs(30);

/// Arguments for the `uninstall` subcommand.
#[derive(Debug, Args)]
pub struct UninstallArgs {
    /// Keep /etc/newrelic-agent-control (useful if you plan to reinstall).
    #[arg(long)]
    pub keep_config: bool,

    /// Show what would be removed without making any changes.
    #[arg(long)]
    pub dry_run: bool,

    /// Skip the confirmation prompt and assume yes to all questions (for automation / CI).
    #[arg(long)]
    pub assume_yes: bool,
}

/// Run the uninstall subcommand.
///
/// Stops the Agent Control daemon, removes all OCI-installed managed agents, removes
/// the binaries and service unit, and (unless --keep-config) removes config/state dirs.
pub fn run(args: UninstallArgs) -> Result<(), CliError> {
    // Root check — every step below requires elevated privileges on a real system.
    #[cfg(target_family = "unix")]
    if !args.dry_run {
        require_root()?;
    }

    if args.dry_run {
        println!("Dry-run: no changes will be made.\n");
    }

    // Confirm before doing anything destructive.
    if !args.dry_run && !args.assume_yes {
        confirm_uninstall(args.keep_config)?;
    }

    stop_service_and_wait(args.dry_run);
    disable_service(args.dry_run);

    // Remove state dir — contains all OCI-installed managed agent packages.
    //
    // WHY NOT use `apt-get remove` / `yum remove` / `zypper remove`?
    // Agent Control installs and updates itself AND its managed agents (infra-agent,
    // NRDOT, OTel collectors) via OCI binary replacement, completely bypassing the
    // system package manager. The package manager only knows about the initial
    // bootstrap package — it has no record of managed agents installed under
    // /var/lib/newrelic-agent-control/. Using `apt-get remove` would silently leave
    // all managed agents running. This command is the only path that removes them.
    //
    // WHY NOT delegate to uninstall.ps1 on Windows?
    // The existing uninstall.ps1 (build/package/windows/uninstall.ps1) stops the
    // Windows service and removes the binary, but does NOT remove OCI-installed managed
    // agent packages. This implementation covers both the service lifecycle AND managed
    // agent cleanup on both platforms.
    remove_path(
        REMOTE_DATA_DIR,
        "state directory (managed agent packages)",
        args.dry_run,
    );

    if !args.keep_config {
        remove_path(LOCAL_DATA_DIR, "config directory", args.dry_run);
    } else {
        info!("Keeping config directory at {LOCAL_DATA_DIR} (--keep-config)");
    }

    remove_binary(AC_BINARY, "Agent Control daemon binary", args.dry_run);
    remove_binary(AC_CLI_BINARY, "Agent Control CLI binary", args.dry_run);

    if args.dry_run {
        println!("\nDry-run complete. Re-run without --dry-run to apply changes.");
    } else {
        println!("Agent Control uninstalled successfully.");
        if args.keep_config {
            println!("Configuration preserved at {LOCAL_DATA_DIR}");
        }
    }

    Ok(())
}

/// Require the process to be running as root (UID 0).
#[cfg(target_family = "unix")]
fn require_root() -> Result<(), CliError> {
    let output = Command::new("id")
        .arg("-u")
        .output()
        .map_err(|e| CliError::Precondition(format!("cannot determine current user: {e}")))?;
    let uid = String::from_utf8_lossy(&output.stdout);
    if uid.trim() != "0" {
        return Err(CliError::Precondition(
            "uninstall requires root privileges. Run with sudo.".into(),
        ));
    }
    Ok(())
}

/// Prompt the user for confirmation before a destructive uninstall.
fn confirm_uninstall(keep_config: bool) -> Result<(), CliError> {
    println!("This will:");
    println!("  • Stop and disable the Agent Control service");
    println!("  • Remove all OCI-installed managed agent packages");
    if !keep_config {
        println!("  • Remove /etc/newrelic-agent-control (pass --keep-config to skip)");
    }
    println!("  • Remove /var/lib/newrelic-agent-control");
    println!("  • Remove the Agent Control binaries");
    println!();
    print!("Continue? [y/N] ");
    std::io::stdout()
        .flush()
        .map_err(|e| CliError::Command(format!("stdout flush: {e}")))?;

    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .map_err(|e| CliError::Command(format!("reading input: {e}")))?;

    if input.trim().eq_ignore_ascii_case("y") || input.trim().eq_ignore_ascii_case("yes") {
        Ok(())
    } else {
        Err(CliError::Command("Uninstall cancelled.".into()))
    }
}

/// Stop the service and poll until it is actually inactive (or timeout).
fn stop_service_and_wait(dry_run: bool) {
    if dry_run {
        println!("[dry-run] Would stop service: {AC_SERVICE_NAME}");
        return;
    }

    #[cfg(target_family = "unix")]
    {
        info!("Stopping service {AC_SERVICE_NAME}");
        let status = Command::new("systemctl")
            .args(["stop", AC_SERVICE_NAME])
            .status();
        match status {
            Ok(s) if s.success() => info!("systemctl stop returned success"),
            Ok(s) => warn!("systemctl stop exited with status {s}; will still wait"),
            Err(e) => warn!("Failed to run systemctl stop: {e}; will still wait"),
        }

        // Wait for the service to become inactive before removing binaries.
        let deadline = Instant::now() + SERVICE_STOP_TIMEOUT;
        loop {
            let active = Command::new("systemctl")
                .args(["is-active", "--quiet", AC_SERVICE_NAME])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);

            if !active {
                info!("Service is no longer active");
                break;
            }

            if Instant::now() >= deadline {
                warn!(
                    "Service did not stop within {}s; proceeding anyway",
                    SERVICE_STOP_TIMEOUT.as_secs()
                );
                break;
            }

            std::thread::sleep(Duration::from_millis(500));
        }
    }

    #[cfg(target_family = "windows")]
    {
        info!("Stopping service {AC_SERVICE_NAME}");
        let status = Command::new("sc").args(["stop", AC_SERVICE_NAME]).status();
        match status {
            Ok(s) if s.success() => info!("Service stopped"),
            Ok(s) => warn!("sc stop exited with status {s}; continuing"),
            Err(e) => warn!("Failed to run sc stop: {e}; continuing"),
        }
        // Give the SCM a moment to clean up.
        std::thread::sleep(Duration::from_secs(2));
    }
}

fn disable_service(dry_run: bool) {
    #[cfg(target_family = "unix")]
    {
        if dry_run {
            println!("[dry-run] Would disable and remove systemd unit: {AC_SERVICE_NAME}");
            return;
        }

        info!("Disabling service {AC_SERVICE_NAME}");
        let _ = Command::new("systemctl")
            .args(["disable", AC_SERVICE_NAME])
            .status()
            .inspect_err(|e| warn!("systemctl disable failed: {e}"));

        if Path::new(SYSTEMD_UNIT).exists() {
            if let Err(e) = std::fs::remove_file(SYSTEMD_UNIT) {
                warn!("Failed to remove {SYSTEMD_UNIT}: {e}");
            } else {
                info!("Removed {SYSTEMD_UNIT}");
            }
        }

        let _ = Command::new("systemctl")
            .args(["daemon-reload"])
            .status()
            .inspect_err(|e| warn!("systemctl daemon-reload failed: {e}"));
    }

    #[cfg(target_family = "windows")]
    {
        if dry_run {
            println!("[dry-run] Would delete Windows service: {AC_SERVICE_NAME}");
            return;
        }
        let _ = Command::new("sc")
            .args(["delete", AC_SERVICE_NAME])
            .status()
            .inspect_err(|e| warn!("sc delete failed: {e}"));
    }
}

fn remove_path(path: &str, description: &str, dry_run: bool) {
    if dry_run {
        println!("[dry-run] Would remove {description}: {path}");
        return;
    }

    let p = Path::new(path);
    if !p.exists() {
        info!("{description} not found, skipping: {path}");
        return;
    }

    let result = if p.is_dir() {
        std::fs::remove_dir_all(p)
    } else {
        std::fs::remove_file(p)
    };

    match result {
        Ok(()) => info!("Removed {description}: {path}"),
        Err(e) => warn!("Failed to remove {description} at {path}: {e}"),
    }
}

fn remove_binary(path: &str, description: &str, dry_run: bool) {
    if dry_run {
        println!("[dry-run] Would remove {description}: {path}");
        return;
    }
    let p = Path::new(path);
    if !p.exists() {
        info!("{description} not found, skipping: {path}");
        return;
    }
    match std::fs::remove_file(p) {
        Ok(()) => info!("Removed {description}: {path}"),
        Err(e) => warn!("Failed to remove {description} at {path}: {e}"),
    }
}
