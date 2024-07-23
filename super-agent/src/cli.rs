mod one_shot_operation;

use clap::Parser;
use one_shot_operation::OneShotCommand;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;
use tracing::info;

use crate::super_agent::defaults::SUPER_AGENT_CONFIG_FILE;
#[cfg(debug_assertions)]
use crate::super_agent::run::set_debug_dirs;
use crate::values::file::YAMLConfigRepositoryFile;
use crate::{
    logging::config::{FileLoggerGuard, LoggingError},
    super_agent::{
        config::SuperAgentConfigError,
        config_patcher::ConfigPatcher,
        config_storer::{loader_storer::SuperAgentConfigLoader, store::SuperAgentConfigStore},
        run::{BasePaths, SuperAgentRunConfig},
    },
    utils::binary_metadata::binary_metadata,
};

/// Represents all the data structures that can be created from the CLI
pub struct SuperAgentCliConfig {
    pub run_config: SuperAgentRunConfig,
    pub file_logger_guard: FileLoggerGuard,
}

#[derive(Debug, Error)]
pub enum CliError {
    #[error("Could not read Super Agent config: `{0}`")]
    ConfigRead(#[from] SuperAgentConfigError),
    #[error("Could not initialize logging: `{0}`")]
    LoggingInit(#[from] LoggingError),
    #[error("k8s config missing while running on k8s ")]
    K8sConfig(),
}

/// What action was requested from the CLI?
pub enum CliCommand {
    /// Normal operation requested. Get the required config and continue.
    InitSuperAgent(SuperAgentCliConfig),
    /// Do an "one-shot" operation and exit successfully.
    /// In the future, many different operations could be added here.
    OneShot(OneShotCommand),
}

#[derive(Parser, Debug)]
#[command(author, about, long_about = None)] // Read from `Cargo.toml`
pub struct Cli {
    #[arg(short, long, default_value_t = String::from("/etc/newrelic-super-agent/config.yaml"))]
    config: String,

    #[arg(long)]
    print_debug_info: bool,

    #[arg(long)]
    version: bool,

    /// Overrides the default local configuration path `/etc/newrelic-super-agent/`.
    /// This config takes precedence over the general `debug`
    #[cfg(debug_assertions)]
    #[arg(long)]
    pub local_dir: Option<PathBuf>,

    /// Overrides the default remote configuration path `/var/lib/newrelic-super-agent`.
    /// This config takes precedence over the general `debug`
    #[cfg(debug_assertions)]
    #[arg(long)]
    pub remote_dir: Option<PathBuf>,

    /// Overrides the default log path `/var/log/newrelic-super-agent`.
    /// This config takes precedence over the general `debug`    
    #[cfg(debug_assertions)]
    #[arg(long)]
    pub logs_dir: Option<PathBuf>,

    /// Overrides the default paths used for local/remote configuration and logs to the following
    /// relatives paths.
    /// `/etc/newrelic-super-agent/` -> <defined path>/nrsa_local
    /// `/var/lib/newrelic-super-agent` -> <defined path>/nrsa_remote
    /// `/var/log/newrelic-super-agent` -> <defined path>/nrsa_logs
    #[cfg(debug_assertions)]
    #[arg(long, value_name = "DATA_DIR")]
    pub debug: Option<PathBuf>,
}

impl Cli {
    /// Parses command line arguments and decides how the application runs
    pub fn init() -> Result<CliCommand, CliError> {
        // Get command line args
        let cli = Self::parse();

        let base_paths = BasePaths::new(cli.config.clone());

        // Initialize debug directories (if set)
        #[cfg(debug_assertions)]
        let base_paths = set_debug_dirs(base_paths, &cli);

        // If the version flag is set, print the version and exit
        if cli.print_version() {
            return Ok(CliCommand::OneShot(OneShotCommand::PrintVersion));
        }
        if cli.print_debug_info() {
            return Ok(CliCommand::OneShot(OneShotCommand::PrintDebugInfo(cli)));
        }

        let vr = YAMLConfigRepositoryFile::new(
            base_paths.super_agent_local_config.clone(),
            base_paths.remote_dir.join(SUPER_AGENT_CONFIG_FILE),
        );

        // In both K8s and onHost we read here the super-agent config that is used to bootstrap the SA from file
        // In the K8s such config is used create the k8s client to create the storer that reads configs from configMaps
        // The real configStores are created in the run fn, the onhost reads file, the k8s one reads configMaps
        let mut super_agent_config = SuperAgentConfigStore::new(Arc::new(vr))
            .load()
            .inspect_err(|err| {
                println!(
                    "Could not read Super Agent config from {}: {}",
                    base_paths.super_agent_local_config.to_string_lossy(),
                    err
                )
            })?;

        let config_patcher =
            ConfigPatcher::new(base_paths.local_dir.clone(), base_paths.log_dir.clone());
        config_patcher.patch(&mut super_agent_config);

        let file_logger_guard = super_agent_config.log.try_init()?;
        info!("{}", binary_metadata());
        info!("Starting NewRelic Super Agent with config '{}'", cli.config);

        let opamp = super_agent_config.opamp;
        let http_server = super_agent_config.server;

        let run_config = SuperAgentRunConfig {
            opamp,
            http_server,
            base_paths,
            #[cfg(feature = "k8s")]
            k8s_config: super_agent_config.k8s.ok_or(CliError::K8sConfig())?,
        };

        let cli_config = SuperAgentCliConfig {
            run_config,
            file_logger_guard,
        };

        Ok(CliCommand::InitSuperAgent(cli_config))
    }

    fn print_version(&self) -> bool {
        self.version
    }

    fn print_debug_info(&self) -> bool {
        self.print_debug_info
    }
}
