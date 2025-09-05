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
use crate::values::file::ConfigRepositoryFile;
use crate::{
    agent_control::{
        config_repository::{repository::AgentControlConfigLoader, store::AgentControlConfigStore},
        run::{AgentControlRunConfig, BasePaths},
    },
    utils::binary_metadata::binary_metadata,
};
use clap::Parser;
use std::error::Error;
use std::process::ExitCode;
use std::sync::Arc;
use tracing::{error, info};

/// All possible errors that can happen while running the initialization.
#[derive(Debug, thiserror::Error)]
pub enum InitError {
    /// Could not initialize tracer
    #[error("could not initialize tracer: {0}")]
    TracerError(#[from] TracingError),
    /// K8s config is missing
    #[error("k8s config missing while running on k8s")]
    K8sConfig(),
    /// The config could not be read
    #[error("could not read Agent Control config from {0}: {1}")]
    LoaderError(String, String),
    /// The configuration is invalid
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
}

/// Command line arguments for Agent Control, as parsed by [`clap`].
#[derive(Parser, Debug)]
#[command(author, about, long_about = None)] // Read from `Cargo.toml`
pub struct Command {
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

impl Command {
    /// Checks if the flag to show the version was set
    fn print_version(&self) -> bool {
        self.version
    }

    /// Checks if the flag to show debug information was set
    fn print_debug_info(&self) -> bool {
        self.print_debug_info
    }

    /// Runs the provided main function or shows the binary information according to flags
    pub fn run<F: Fn(AgentControlRunConfig, Vec<TracingGuardBox>) -> Result<(), Box<dyn Error>>>(
        mode: Environment,
        main_fn: F,
    ) -> ExitCode {
        // Get command line args
        let flags = Self::parse();

        let base_paths = BasePaths::default();

        // Initialize debug directories (if set)
        #[cfg(debug_assertions)]
        let base_paths = set_debug_dirs(base_paths, &flags);

        // Handle flags requiring different execution mode
        if flags.print_version() {
            println!("{}", binary_metadata(mode));
            return ExitCode::SUCCESS;
        }
        if flags.print_debug_info() {
            println!("Printing debug info");
            println!("Agent Control Mode: {mode:?}");
            println!("FLAGS: {flags:#?}");
            return ExitCode::SUCCESS;
        }

        let Ok((run_config, tracer)) =
            Self::init_agent_control(mode, base_paths).inspect_err(|err| {
                // Using print because the tracer might have failed to start
                println!("Error on Agent Control initialization: {err}");
            })
        else {
            return ExitCode::FAILURE;
        };

        match main_fn(run_config, tracer) {
            Ok(_) => {
                info!("The agent control main process exited successfully");
                ExitCode::SUCCESS
            }
            Err(err) => {
                error!("The agent control main process exited with an error: {err}");
                ExitCode::FAILURE
            }
        }
    }

    /// Builds the Agent Control configuration required to execute the application.
    fn init_agent_control(
        mode: Environment,
        base_paths: BasePaths,
    ) -> Result<(AgentControlRunConfig, Vec<TracingGuardBox>), InitError> {
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
        let agent_type_var_constraints = agent_control_config.agent_type_var_constraints;

        let run_config = AgentControlRunConfig {
            opamp,
            http_server,
            base_paths,
            proxy,

            k8s_config: match mode {
                // This config is not used on the OnHost environment, a blank config is used.
                // K8sConfig has not "default" since cluster_name is a required.
                Environment::OnHost => K8sConfig::default(),
                Environment::K8s => agent_control_config.k8s.ok_or(InitError::K8sConfig())?,
            },
            agent_type_var_constraints,
        };
        Ok((run_config, tracer))
    }
}
