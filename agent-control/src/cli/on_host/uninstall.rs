use crate::cli::common::error::CliError;
use clap::Args;
use std::path::Path;
use std::process::Command;
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

/// Arguments for the `uninstall` subcommand.
#[derive(Debug, Args)]
pub struct UninstallArgs {
    /// Keep /etc/newrelic-agent-control (useful if you plan to reinstall).
    #[arg(long)]
    pub keep_config: bool,

    /// Show what would be removed without making any changes.
    #[arg(long)]
    pub dry_run: bool,
}

/// Run the uninstall subcommand.
///
/// Stops the Agent Control daemon, removes all OCI-installed managed agents, removes
/// the binaries and service unit, and (unless --keep-config) removes config/state dirs.
pub fn run(args: UninstallArgs) -> Result<(), CliError> {
    if args.dry_run {
        println!("Dry-run: no changes will be made.\n");
    }

    stop_service(args.dry_run);
    disable_service(args.dry_run);

    // Remove state dir — contains all OCI-installed managed agent packages.
    // These agents are NOT tracked by the system package manager and must be
    // removed explicitly here.
    remove_path(REMOTE_DATA_DIR, "state directory (managed agent packages)", args.dry_run);

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

fn stop_service(dry_run: bool) {
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
            Ok(s) if s.success() => info!("Service stopped"),
            Ok(s) => warn!("systemctl stop exited with status {s}; continuing"),
            Err(e) => warn!("Failed to run systemctl stop: {e}; continuing"),
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
