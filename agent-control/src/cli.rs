mod one_shot_operation;
#[cfg(debug_assertions)]
use crate::agent_control::run::set_debug_dirs;
use crate::opamp::client_builder::DEFAULT_POLL_INTERVAL;
use crate::values::file::YAMLConfigRepositoryFile;
use crate::{
    agent_control::{
        config::AgentControlConfigError,
        config_storer::{loader_storer::AgentControlConfigLoader, store::AgentControlConfigStore},
        run::{AgentControlRunConfig, BasePaths},
    },
    logging::config::{FileLoggerGuard, LoggingError},
    utils::binary_metadata::binary_metadata,
};
use clap::Parser;
use one_shot_operation::OneShotCommand;
use std::sync::Arc;
use thiserror::Error;
use tracing::info;

/// Represents all the data structures that can be created from the CLI
pub struct AgentControlCliConfig {
    pub run_config: AgentControlRunConfig,
    pub file_logger_guard: FileLoggerGuard,
}

#[derive(Debug, Error)]
pub enum CliError {
    #[error("Could not read Agent Control config: `{0}`")]
    ConfigRead(#[from] AgentControlConfigError),
    #[error("Could not initialize logging: `{0}`")]
    LoggingInit(#[from] LoggingError),
    #[error("k8s config missing while running on k8s ")]
    K8sConfig(),
    #[error("Could not read Agent Control config from `{0}`: `{1}`")]
    LoaderError(String, String),
    #[error("Invalid configuration: `{0}`")]
    InvalidConfig(String),
}

/// What action was requested from the CLI?
pub enum CliCommand {
    /// Normal operation requested. Get the required config and continue.
    InitAgentControl(AgentControlCliConfig),
    /// Do an "one-shot" operation and exit successfully.
    /// In the future, many different operations could be added here.
    OneShot(OneShotCommand),
}

#[derive(Parser, Debug)]
#[command(author, about, long_about = None)] // Read from `Cargo.toml`
pub struct Cli {
    #[arg(long)]
    print_debug_info: bool,

    #[arg(long)]
    version: bool,

    /// Overrides the default local configuration path `/etc/newrelic-agent-control/`.
    #[cfg(debug_assertions)]
    #[arg(long)]
    pub local_dir: Option<std::path::PathBuf>,

    /// Overrides the default remote configuration path `/var/lib/newrelic-agent-control`.
    #[cfg(debug_assertions)]
    #[arg(long)]
    pub remote_dir: Option<std::path::PathBuf>,

    /// Overrides the default log path `/var/log/newrelic-agent-control`.
    #[cfg(debug_assertions)]
    #[arg(long)]
    pub logs_dir: Option<std::path::PathBuf>,
}

impl Cli {
    /// Parses command line arguments and decides how the application runs
    pub fn init() -> Result<CliCommand, CliError> {
        // Get command line args
        let cli = Self::parse();

        let base_paths = BasePaths::default();

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

        let agent_control_repository = YAMLConfigRepositoryFile::new(
            base_paths.local_dir.clone(),
            base_paths.remote_dir.clone(),
        );

        // In both K8s and onHost we read here the agent-control config that is used to bootstrap the SA from file
        // In the K8s such config is used create the k8s client to create the storer that reads configs from configMaps
        // The real configStores are created in the run fn, the onhost reads file, the k8s one reads configMaps
        let agent_control_config = AgentControlConfigStore::new(Arc::new(agent_control_repository))
            .load()
            .map_err(|err| {
                CliError::LoaderError(
                    base_paths.local_dir.to_string_lossy().to_string(),
                    err.to_string(),
                )
            })?;

        let file_logger_guard = agent_control_config
            .log
            .try_init(base_paths.log_dir.clone())?;
        info!("{}", binary_metadata());
        info!(
            "Starting NewRelic Agent Control with config folder '{}'",
            base_paths.local_dir.to_string_lossy().to_string()
        );

        let opamp = agent_control_config.fleet_control.filter(|fc| fc.enabled);
        let http_server = agent_control_config.server;
        let proxy = agent_control_config
            .proxy
            .try_with_url_from_env()
            .map_err(|err| CliError::InvalidConfig(err.to_string()))?;

        let run_config = AgentControlRunConfig {
            opamp,
            opamp_poll_interval: DEFAULT_POLL_INTERVAL,
            http_server,
            base_paths,
            proxy,
            #[cfg(feature = "k8s")]
            k8s_config: agent_control_config.k8s.ok_or(CliError::K8sConfig())?,

            // TODO - Temporal solution until https://new-relic.atlassian.net/browse/NR-343594 is done.
            // There is a current issue with the diff computation the GC does in order to collect agents. If a new agent is added and removed
            // before the GC process it, the resources will never be collected.
            #[cfg(feature = "k8s")]
            garbage_collector_interval: DEFAULT_POLL_INTERVAL - std::time::Duration::from_secs(5),
        };

        let cli_config = AgentControlCliConfig {
            run_config,
            file_logger_guard,
        };

        Ok(CliCommand::InitAgentControl(cli_config))
    }

    fn print_version(&self) -> bool {
        self.version
    }

    fn print_debug_info(&self) -> bool {
        self.print_debug_info
    }
}
