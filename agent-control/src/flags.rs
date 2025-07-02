//! Command line interface for the agent control.
//!
//! Parses the command line arguments and decides how the application runs as defined in [Command].
#![warn(missing_docs)]

use crate::agent_control::config::K8sConfig;
use crate::agent_control::run::Environment;
#[cfg(debug_assertions)]
use crate::agent_control::run::set_debug_dirs;
use crate::instrumentation::tracing::{
    TracingConfig, TracingError, TracingGuardBox, try_init_tracing,
};
use crate::opamp::client_builder::DEFAULT_POLL_INTERVAL;
use crate::values::file::ConfigRepositoryFile;
use crate::{
    agent_control::{
        config_repository::{repository::AgentControlConfigLoader, store::AgentControlConfigStore},
        run::{AgentControlRunConfig, BasePaths},
    },
    utils::binary_metadata::binary_metadata,
};
use clap::Parser;
use std::sync::Arc;
use thiserror::Error;
use tracing::info;

/// All possible errors that can happen while running the initialization.
#[derive(Debug, Error)]
pub enum InitError {
    /// Could not initialize tracer
    #[error("Could not initialize tracer: `{0}`")]
    TracerError(#[from] TracingError),
    /// K8s config is missing
    #[error("k8s config missing while running on k8s ")]
    K8sConfig(),
    /// The config could not be read
    #[error("Could not read Agent Control config from `{0}`: `{1}`")]
    LoaderError(String, String),
    /// The configuration is invalid
    #[error("Invalid configuration: `{0}`")]
    InvalidConfig(String),
}

/// What action was requested from the initialization?
pub enum Command {
    /// Normal operation requested. Get the required config and continue.
    InitAgentControl(AgentControlRunConfig, Vec<TracingGuardBox>),
    /// Do a "one-shot" operation and exit successfully.
    /// In the future, many different operations could be added here.
    OneShot(OneShotCommand),
}

/// Command line arguments for Agent Control, as parsed by [`clap`].
#[derive(Parser, Debug)]
#[command(author, about, long_about = None)] // Read from `Cargo.toml`
pub struct Flags {
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

impl Flags {
    /// Parses command line arguments and decides how the application runs.
    pub fn init(mode: Environment) -> Result<Command, InitError> {
        // Get command line args
        let flags = Self::parse();

        let base_paths = BasePaths::default();

        // Initialize debug directories (if set)
        #[cfg(debug_assertions)]
        let base_paths = set_debug_dirs(base_paths, &flags);

        // If the version flag is set, print the version and exit
        if flags.print_version() {
            return Ok(Command::OneShot(OneShotCommand::PrintVersion));
        }
        if flags.print_debug_info() {
            return Ok(Command::OneShot(OneShotCommand::PrintDebugInfo(flags)));
        }

        let agent_control_repository =
            ConfigRepositoryFile::new(base_paths.local_dir.clone(), base_paths.remote_dir.clone());

        // In both K8s and onHost we read here the agent-control config that is used to bootstrap the SA from file
        // In the K8s such config is used create the k8s client to create the storer that reads configs from configMaps
        // The real configStores are created in the run fn, the onhost reads file, the k8s one reads configMaps
        let agent_control_config = AgentControlConfigStore::new(Arc::new(agent_control_repository))
            .load()
            .map_err(|err| {
                InitError::LoaderError(
                    base_paths.local_dir.to_string_lossy().to_string(),
                    err.to_string(),
                )
            })?;

        let proxy = agent_control_config
            .proxy
            .try_with_url_from_env()
            .map_err(|err| InitError::InvalidConfig(err.to_string()))?;

        let tracing_config = TracingConfig::from_logging_path(base_paths.log_dir.clone())
            .with_logging_config(agent_control_config.log)
            .with_instrumentation_config(
                agent_control_config
                    .self_instrumentation
                    .with_proxy_config(proxy.clone()),
            );
        let tracer = try_init_tracing(tracing_config)?;

        info!("{}", binary_metadata(mode));
        info!(
            "Starting NewRelic Agent Control with config folder '{}'",
            base_paths.local_dir.to_string_lossy().to_string()
        );

        let opamp = agent_control_config.fleet_control;
        let http_server = agent_control_config.server;

        let run_config = AgentControlRunConfig {
            opamp,
            opamp_poll_interval: DEFAULT_POLL_INTERVAL,
            http_server,
            base_paths,
            proxy,

            k8s_config: match mode {
                // This config is not used on the OnHost environment, a blank config is used.
                // K8sConfig has not "default" since cluster_name is a required.
                Environment::OnHost => K8sConfig::default(),
                Environment::K8s => agent_control_config.k8s.ok_or(InitError::K8sConfig())?,
            },
        };

        Ok(Command::InitAgentControl(run_config, tracer))
    }

    fn print_version(&self) -> bool {
        self.version
    }

    fn print_debug_info(&self) -> bool {
        self.print_debug_info
    }
}

/// One-shot operations that can be performed by the agent-control
pub enum OneShotCommand {
    /// Print the version of the agent-control and exits
    PrintVersion,
    /// Print debug information and exits
    PrintDebugInfo(Flags),
}

impl OneShotCommand {
    /// Runs the one-shot operation
    pub fn run_one_shot(&self, env: Environment) {
        match self {
            OneShotCommand::PrintVersion => {
                println!("{}", binary_metadata(env));
            }
            OneShotCommand::PrintDebugInfo(flags) => {
                println!("Printing debug info");
                println!("Agent Control Mode: {env:?}");
                println!("FLAGS: {:#?}", flags);
            }
        }
    }
}
