use crate::agent_control::agent_id::AgentID;
use crate::agent_control::config::Registry;
use crate::agent_control::defaults::{
    AC_OCI_PACKAGE_DEFAULT_REPOSITORY, AC_OCI_PACKAGE_PUBLIC_KEY_URL, AGENT_CONTROL_VERSION,
};
use crate::agent_control::version_updater::on_host::{
    AGENT_CONTROL_BIN, AGENT_CONTROL_BIN_PACKAGE_ID,
};
use crate::agent_type::runtime_config::on_host::package::rendered::{Oci, Repository, Version};
use crate::cli::common::error::CliError;
use crate::http::config::ProxyConfig;
use crate::oci::Client;
use crate::package::manager::{PackageData, PackageManager};
use crate::package::oci::downloader::OCIPackageArtifactDownloader;
use crate::package::oci::package_manager::OCIPackageManager;
use clap::Args;
use fs::directory_manager::DirectoryManagerFs;
use oci_client::client::ClientConfig;
use self_replacer::{BinarySelfReplacer, SelfReplacer};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tracing::info;
use url::Url;

const REMOTE_DATA_DIR: &str = "/var/lib/newrelic-agent-control";

/// Arguments for the `update` subcommand.
#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// Target version to install, e.g. `1.16.0`. Must be a valid semver string.
    #[arg(long)]
    pub version: String,

    /// Show what would happen without making any changes.
    #[arg(long)]
    pub dry_run: bool,

    /// Skip OCI signature verification. Not recommended for production use.
    #[arg(long, hide = true)]
    pub skip_verify: bool,
}

/// Run the update subcommand.
///
/// Downloads the requested Agent Control version from the OCI registry and
/// atomically self-replaces the running binary. Agent Control must be running
/// as a systemd service so that the `Restart=always` policy brings up the new
/// version after this process exits.
///
/// This command bypasses Fleet Control release channels and is intended only
/// as a break-glass operation for installations not managed by Fleet Control.
pub fn run(args: UpdateArgs) -> Result<(), CliError> {
    let version = Version::from_str(&args.version)
        .map_err(|e| CliError::Command(format!("invalid version '{}': {e}", args.version)))?;

    let repository = Repository::from_str(AC_OCI_PACKAGE_DEFAULT_REPOSITORY)
        .map_err(|e| CliError::Command(format!("invalid OCI repository: {e}")))?;

    let public_key_url = Url::parse(AC_OCI_PACKAGE_PUBLIC_KEY_URL)
        .map_err(|e| CliError::Command(format!("invalid public key URL: {e}")))?;

    let signature_verification = !args.skip_verify;

    if args.dry_run {
        let idempotent = version.to_string() == AGENT_CONTROL_VERSION;
        if idempotent {
            println!("Agent Control is already at version {version} — this would be a no-op.");
        } else {
            println!(
                "Dry-run: would download Agent Control {version} from \
                 {AC_OCI_PACKAGE_DEFAULT_REPOSITORY} (sig-verify={signature_verification}) \
                 and self-replace the running binary."
            );
        }
        return Ok(());
    }

    // Idempotency: skip if already at the requested version.
    if version.to_string() == AGENT_CONTROL_VERSION {
        println!("Agent Control is already at version {version}. Nothing to do.");
        return Ok(());
    }

    if args.skip_verify {
        tracing::warn!("Signature verification disabled via --skip-verify. Use with caution.");
    }

    info!("Downloading Agent Control {version} from OCI registry");

    let package_data = PackageData {
        id: AGENT_CONTROL_BIN_PACKAGE_ID.to_string(),
        oci: Oci {
            repository,
            version: version.clone(),
            public_key_url: Some(public_key_url),
        },
        post_download_hook: None,
    };

    let runtime = Arc::new(
        Runtime::new()
            .map_err(|e| CliError::Command(format!("failed to create async runtime: {e}")))?,
    );

    let oci_client = Client::try_new(ClientConfig::default(), ProxyConfig::default(), runtime)
        .map_err(|e| CliError::Command(format!("failed to create OCI client: {e}")))?;

    let downloader = OCIPackageArtifactDownloader::new(
        oci_client,
        Registry::default(),
        None, // no auth — public registry
        signature_verification,
    );

    let remote_dir = PathBuf::from(REMOTE_DATA_DIR);
    let package_manager = OCIPackageManager::new(downloader, DirectoryManagerFs, remote_dir);

    let installed = package_manager
        .install(&AgentID::AgentControl, package_data)
        .map_err(|e| CliError::Command(format!("OCI install failed: {e}")))?;

    let new_binary = installed.installation_path.join(AGENT_CONTROL_BIN);

    // Verify the downloaded binary is executable before self-replacing.
    #[cfg(target_family = "unix")]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata(&new_binary)
            .map_err(|e| CliError::Command(format!("downloaded binary not readable: {e}")))?;
        if meta.permissions().mode() & 0o111 == 0 {
            return Err(CliError::Command(
                "downloaded binary has no execute permission".into(),
            ));
        }
    }

    info!(
        "Binary downloaded to {}. Performing self-replace.",
        new_binary.display()
    );

    BinarySelfReplacer::self_replace(&new_binary)
        .map_err(|e| CliError::Command(format!("self-replace failed: {e}")))?;

    // The binary has been replaced. Exit so that systemd (Restart=always) starts
    // the new version.
    println!("Agent Control {version} installed. Restarting via systemd.");
    std::process::exit(0);
}
