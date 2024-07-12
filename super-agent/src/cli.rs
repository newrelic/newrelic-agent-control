mod one_shot_operation;

use clap::Parser;
use one_shot_operation::OneShotCommand;
use std::path::PathBuf;
use thiserror::Error;
use tracing::info;

#[cfg(debug_assertions)]
use crate::super_agent::run::set_debug_dirs;
use crate::{
    logging::config::{FileLoggerGuard, LoggingError},
    super_agent::{
        config::SuperAgentConfigError,
        config_patcher::ConfigPatcher,
        config_storer::{loader_storer::SuperAgentConfigLoader, store::SuperAgentConfigStore},
        defaults::{SUPER_AGENT_DATA_DIR, SUPER_AGENT_LOCAL_DATA_DIR},
        run::SuperAgentRunConfig,
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

        // Initialize debug directories (if set)
        #[cfg(debug_assertions)]
        set_debug_dirs(&cli);

        // If the version flag is set, print the version and exit
        if cli.print_version() {
            return Ok(CliCommand::OneShot(OneShotCommand::PrintVersion));
        }
        if cli.print_debug_info() {
            return Ok(CliCommand::OneShot(OneShotCommand::PrintDebugInfo(cli)));
        }

        // TODO: check if we need some sort of shared reference for SuperAgentPaths
        let remote_dir = PathBuf::from(SUPER_AGENT_DATA_DIR());
        let config_storer = SuperAgentConfigStore::new(&cli.get_config_path(), remote_dir);

        let mut super_agent_config = config_storer.load().inspect_err(|err| {
            println!(
                "Could not read Super Agent config from {}: {}",
                config_storer.config_path().to_string_lossy(),
                err
            )
        })?;

        let config_patcher = ConfigPatcher::new(SUPER_AGENT_LOCAL_DATA_DIR());
        config_patcher.patch(&mut super_agent_config);

        let file_logger_guard = super_agent_config.log.try_init()?;
        info!("{}", binary_metadata());
        info!(
            "Starting NewRelic Super Agent with config '{}'",
            config_storer.config_path().to_string_lossy()
        );

        let opamp = super_agent_config.opamp;
        let http_server = super_agent_config.server;

        let run_config = SuperAgentRunConfig {
            config_storer,
            opamp,
            http_server,
        };

        let cli_config = SuperAgentCliConfig {
            run_config,
            file_logger_guard,
        };

        Ok(CliCommand::InitSuperAgent(cli_config))
    }

    fn get_config_path(&self) -> PathBuf {
        PathBuf::from(&self.config)
    }

    fn print_version(&self) -> bool {
        self.version
    }

    fn print_debug_info(&self) -> bool {
        self.print_debug_info
    }
}
