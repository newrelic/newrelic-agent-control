use clap::{Parser, Subcommand};
use newrelic_agent_control::cli::errors::CliError;
use newrelic_agent_control::cli::install_agent_control::{
    AgentControlInstallData, install_agent_control,
};
use newrelic_agent_control::cli::uninstall_agent_control::{
    AgentControlUninstallData, uninstall_agent_control,
};
use newrelic_agent_control::{
    agent_control::defaults::AGENT_CONTROL_LOG_DIR,
    http::tls::install_rustls_default_crypto_provider,
    instrumentation::{
        config::logs::config::LoggingConfig,
        tracing::{TracingConfig, try_init_tracing},
    },
};
use std::{path::PathBuf, process::ExitCode};
use tracing::{Level, debug, error};

/// Manage agent control resources
#[derive(Debug, Parser)]
#[command()]
struct Cli {
    #[command(subcommand)]
    operation: Operations,

    /// Namespace where the operation will be performed
    #[arg(short, long, global = true, default_value = "default")]
    namespace: String,

    /// Log level upperbound
    #[arg(long, global = true, default_value = "info")]
    log_level: Level,
}

#[derive(Debug, Subcommand)]
enum Operations {
    /// Install agent control chart and create required resources
    InstallAgentControl(AgentControlInstallData),
    /// Uninstall agent control and delete related resources
    UninstallAgentControl(AgentControlUninstallData),
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let logging_config: LoggingConfig = serde_yaml::from_str(&format!("level: {}", cli.log_level))
        .expect("Logging config should be valid");
    let tracing_config = TracingConfig::from_logging_path(PathBuf::from(AGENT_CONTROL_LOG_DIR))
        .with_logging_config(logging_config);
    let tracer = try_init_tracing(tracing_config).map_err(|err| CliError::Tracing(err.to_string()));

    if let Err(err) = tracer {
        eprintln!("Failed to initialize tracing: {err:?}");
        return err.to_exit_code();
    }

    debug!("Installing default rustls crypto provider");
    install_rustls_default_crypto_provider();

    let result = match cli.operation {
        Operations::InstallAgentControl(agent_control_data) => {
            install_agent_control(agent_control_data, cli.namespace)
        }
        Operations::UninstallAgentControl(agent_control_data) => {
            uninstall_agent_control(agent_control_data, cli.namespace)
        }
    };

    match result {
        Ok(_) => ExitCode::SUCCESS,
        Err(err) => {
            error!("Operation failed: {}", err);
            err.to_exit_code()
        }
    }
}
