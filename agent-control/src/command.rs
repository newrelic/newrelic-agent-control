//! Command line interface for the agent control.
//!
//! Parses the command line arguments and decides how the application runs as defined in [Command].
#![warn(missing_docs)]

use crate::agent_control::config::K8sConfig;
use crate::agent_control::defaults::ENVIRONMENT_VARIABLES_FILE_NAME;
use crate::agent_control::run::Environment;
use crate::agent_control::{
    config_repository::{repository::AgentControlConfigLoader, store::AgentControlConfigStore},
    run::{AgentControlRunConfig, BasePaths},
};
use crate::event::ApplicationEvent;
use crate::event::channel::{EventConsumer, EventPublisher, pub_sub};
use crate::instrumentation::tracing::{TracingConfig, TracingGuardBox, try_init_tracing};
use crate::on_host::file_store::FileStore;
use crate::utils::binary_metadata::binary_metadata;
use crate::utils::env_var::load_env_yaml_file;
use crate::values::ConfigRepo;
use clap::Parser;
use std::error::Error;
use std::process::ExitCode;
use std::sync::Arc;
use tracing::{error, info};

#[cfg(target_os = "windows")]
pub mod windows;

/// All possible errors that can happen while running the initialization.
#[derive(Debug, thiserror::Error)]
pub enum InitError {
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

/// Available commands for Agent Control
#[derive(Parser, Debug)]
#[command(author, about, long_about = None)] // Read from `Cargo.toml`
pub enum Command {
    /// Run the agent control (default command)
    Run(Args),
    /// Print version information
    Version,
    /// Verify the agent control configuration and ability to be run
    Verify,
}

/// Args contains the list of available args for the agentControl run command
#[derive(Debug, Default, clap::Parser)]
pub struct Args {
    /// Overrides the default local configuration path `/etc/newrelic-agent-control/`.
    #[cfg(debug_assertions)]
    #[arg(long)]
    local_dir: Option<std::path::PathBuf>,

    /// Overrides the default remote configuration path `/var/lib/newrelic-agent-control`.
    #[cfg(debug_assertions)]
    #[arg(long)]
    remote_dir: Option<std::path::PathBuf>,

    /// Overrides the default log path `/var/log/newrelic-agent-control`.
    #[cfg(debug_assertions)]
    #[arg(long)]
    logs_dir: Option<std::path::PathBuf>,
}

/// Context passed to the main loop, containing all initialized components.
pub struct RunContext {
    /// Configuration for the runner
    pub run_config: AgentControlRunConfig,
    /// This must be kept alive for the duration of the program to ensure logs and traces are flushed.
    pub tracer: Vec<TracingGuardBox>,
    /// The consuming end of the internal application event bus.
    pub application_event_consumer: EventConsumer<ApplicationEvent>,
    /// A handler used to signal the application to stop when running as a Windows Service
    #[cfg(target_family = "windows")]
    pub stop_handler: Option<windows::WindowsServiceStopHandler>,
}

impl Default for Command {
    // To assure backward compatibility, if no command is provided, we default to Run command.
    fn default() -> Self {
        Command::Run(Args::default())
    }
}

impl Command {
    /// Runs the provided main function or shows the binary information according to commands
    pub fn execute<F>(
        ac_running_mode: Environment,
        main_fn: F,
        #[cfg(target_os = "windows")] as_windows_service: bool,
    ) -> ExitCode
    where
        F: FnOnce(RunContext) -> Result<(), Box<dyn Error>>,
    {
        // For backward compatibility, default to Run command if no subcommand is provided
        let command = if std::env::args().len() == 1 {
            // No arguments provided, default to Run
            Command::default()
        } else {
            // Parse normally, which handles -h, --help,
            Command::parse()
        };

        // Handle commands that don't require full initialization
        match &command {
            Command::Version => Command::print_version(ac_running_mode),
            Command::Verify => {
                //todo
                ExitCode::SUCCESS
            }
            Command::Run(args) => Command::run(
                ac_running_mode,
                main_fn,
                args,
                #[cfg(target_os = "windows")]
                as_windows_service,
            ),
        }
    }

    /// Handles the version command
    fn print_version(ac_running_mode: Environment) -> ExitCode {
        println!("{}", binary_metadata(ac_running_mode));
        ExitCode::SUCCESS
    }

    /// Handles the run command
    fn run<F>(
        ac_running_mode: Environment,
        main_fn: F,
        args: &Args,
        #[cfg(target_os = "windows")] as_windows_service: bool,
    ) -> ExitCode
    where
        F: FnOnce(RunContext) -> Result<(), Box<dyn Error>>,
    {
        match Command::build_run_context(
            ac_running_mode,
            args,
            #[cfg(target_os = "windows")]
            as_windows_service,
        ) {
            Err(err) => {
                // We are leveraging println here instead of error! because if we fail to build the run context,
                // it means we probably failed before initializing tracing, so we can't guarantee that the error will be logged.
                println!("Failed building the run context {}", err);
                ExitCode::FAILURE
            }
            Ok(run_context) => match main_fn(run_context) {
                Ok(_) => {
                    info!("The agent control main process exited successfully");
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    error!("The agent control main process exited with an error: {err}");
                    ExitCode::FAILURE
                }
            },
        }
    }

    /// Builds the complete RunContext required to execute the Run command
    fn build_run_context(
        ac_running_mode: Environment,
        args: &Args,
        #[cfg(target_os = "windows")] as_windows_service: bool,
    ) -> Result<RunContext, Box<dyn Error>> {
        let base_paths = BasePaths::default();

        // Initialize debug directories (if set)
        #[cfg(debug_assertions)]
        let base_paths = set_debug_dirs(base_paths, args);

        // We need to create the pub_sub here so the Windows Service Stop handler is capable
        // of publishing a stop signal to the application for a Graceful Shutdown.
        let (application_event_publisher, application_event_consumer) = pub_sub();

        create_shutdown_signal_handler(application_event_publisher.clone())
            .map_err(|e| format!("Failed to create shutdown signal handler: {e}"))?;

        #[cfg(target_family = "windows")]
        let stop_handler = as_windows_service
            .then(|| windows::setup_windows_service(application_event_publisher))
            .transpose()
            .map_err(|e| format!("Failed to setup Windows service: {e}"))?;

        let env_file_path = base_paths.local_dir.join(ENVIRONMENT_VARIABLES_FILE_NAME);
        if env_file_path.exists() {
            println!(
                "Loading environment variables from: {}",
                env_file_path.display()
            );
            load_env_yaml_file(env_file_path.as_path())
                .map_err(|e| format!("Failed to load environment: {e}"))?;
        }

        let (run_config, tracing_config) = Self::build_run_config(ac_running_mode, base_paths)?;

        let tracer = try_init_tracing(tracing_config)
            .map_err(|e| format!("Error on Agent Control tracing initialization: {e}"))?;

        info!("{}", binary_metadata(run_config.ac_running_mode));
        info!(
            "Starting NewRelic Agent Control with config folder '{}'",
            run_config.base_paths.local_dir.to_string_lossy()
        );

        Ok(RunContext {
            run_config,
            tracer,
            application_event_consumer,
            #[cfg(target_family = "windows")]
            stop_handler,
        })
    }

    /// Builds the Agent Control configuration required to execute the application.
    fn build_run_config(
        ac_running_mode: Environment,
        base_paths: BasePaths,
    ) -> Result<(AgentControlRunConfig, TracingConfig), InitError> {
        let file_store = Arc::new(FileStore::new_local_fs(
            base_paths.local_dir.clone(),
            base_paths.remote_dir.clone(),
        ));
        // AC config is treated as other agents configs and the location of the local config file follows the same
        // fs layout. Example for linux is expected to be in '/etc/newrelic-agent-control/local-data/agent-control/'
        // In both K8s and onHost we read here the agent-control config that is used to bootstrap the AC from file.
        // In the K8s such config is used create the k8s client to create the storer that reads configs from configMaps.
        // The real configStores are created in the run fn, the onhost reads file, the k8s one reads configMaps.
        let agent_control_config_repository = ConfigRepo::new(file_store);
        let agent_control_config =
            AgentControlConfigStore::new(Arc::new(agent_control_config_repository))
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

        let opamp = agent_control_config.fleet_control;
        let http_server = agent_control_config.server;
        let agent_type_var_constraints = agent_control_config.agent_type_var_constraints;

        let run_config = AgentControlRunConfig {
            ac_running_mode,
            opamp,
            http_server,
            base_paths,
            proxy,
            k8s_config: match ac_running_mode {
                // This config is not used on the OnHost environment, a blank config is used.
                // K8sConfig has not "default" since cluster_name is a required.
                Environment::K8s => agent_control_config.k8s.ok_or(InitError::K8sConfig())?,
                _ => K8sConfig::default(),
            },
            agent_type_var_constraints,
        };
        Ok((run_config, tracing_config))
    }
}

/// Enables using the typical keypress (Ctrl-C) to stop the agent control process at any moment.
///
/// This means sending [ApplicationEvent::StopRequested] to the agent control event processor
/// so it can release all resources.
fn create_shutdown_signal_handler(
    publisher: EventPublisher<ApplicationEvent>,
) -> Result<(), ctrlc::Error> {
    ctrlc::set_handler(move || {
        info!("Received SIGINT (Ctrl-C). Stopping agent control");
        let _ = publisher
            .publish(ApplicationEvent::StopRequested)
            .inspect_err(|e| error!("Could not send agent control stop request: {}", e));
    })
    .inspect_err(|e| error!("Could not set signal handler: {e}"))
}

#[cfg(debug_assertions)]
/// Set path override if local_dir, remote_dir, and logs_dir flags are set
fn set_debug_dirs(base_paths: BasePaths, args: &Args) -> BasePaths {
    let mut base_paths = base_paths;

    if let Some(ref local_path) = args.local_dir {
        base_paths.local_dir = local_path.to_path_buf();
    }
    if let Some(ref remote_path) = args.remote_dir {
        base_paths.remote_dir = remote_path.to_path_buf();
    }
    if let Some(ref log_path) = args.logs_dir {
        base_paths.log_dir = log_path.to_path_buf();
    }

    base_paths
}
