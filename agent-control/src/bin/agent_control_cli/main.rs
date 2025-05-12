use std::{path::PathBuf, process::ExitCode, sync::Arc};

use agent_control::AgentControlData;
use clap::{Parser, Subcommand};
use errors::CliError;
use kube::{Resource, api::DynamicObject};
use newrelic_agent_control::{
    agent_control::defaults::AGENT_CONTROL_LOG_DIR,
    http::tls::install_rustls_default_crypto_provider,
    instrumentation::{
        config::logs::config::LoggingConfig,
        tracing::{TracingConfig, try_init_tracing},
    },
    k8s::client::SyncK8sClient,
};
use tracing::{Level, debug, error, info};

mod agent_control;
mod errors;
mod utils;

/// Manage Helm releases and repositories in Kubernetes.
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
    /// Install a helm chart and create required resources
    Install {
        #[command(subcommand)]
        application: Application,
    },
}

#[derive(Debug, Subcommand)]
enum Application {
    /// Operate over an application
    AgentControl(AgentControlData),
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let logging_config: LoggingConfig = serde_yaml::from_str(&format!("level: {}", cli.log_level))
        .expect("Logging config should be valid");
    let tracing_config = TracingConfig::from_logging_path(PathBuf::from(AGENT_CONTROL_LOG_DIR))
        .with_logging_config(logging_config);
    let tracer = try_init_tracing(tracing_config).map_err(CliError::Tracing);

    if let Err(err) = tracer {
        eprintln!("Failed to initialize tracing: {:?}", err);
        return err.to_exit_code();
    }

    debug!("Installing default rustls crypto provider");
    install_rustls_default_crypto_provider();

    let result = match cli.operation {
        Operations::Install { application } => match application {
            Application::AgentControl(agent_control) => {
                install_agent_control(agent_control, cli.namespace)
            }
        },
    };

    match result {
        Ok(_) => ExitCode::SUCCESS,
        Err(err) => {
            error!("Operation failed: {:?}", err);
            err.to_exit_code()
        }
    }
}

fn install_agent_control(data: AgentControlData, namespace: String) -> Result<(), CliError> {
    info!("Installing agent control");

    let dynamic_objects = Vec::<DynamicObject>::from(data);

    let k8s_client = k8s_client(namespace.clone())?;
    info!("Applying agent control resources");
    for object in dynamic_objects {
        apply_resource(&k8s_client, &object)?;
    }
    info!("Agent control resources applied successfully");

    info!("Agent control installed successfully");

    Ok(())
}

fn apply_resource(k8s_client: &SyncK8sClient, object: &DynamicObject) -> Result<(), CliError> {
    let name = object.meta().name.clone().expect("Name should be present");
    let kind = object
        .types
        .clone()
        .map(|t| t.kind)
        .unwrap_or_else(|| "Unknown kind".to_string());

    info!("Applying \"{}\" with name \"{}\"", kind, name);
    k8s_client
        .apply_dynamic_object(object)
        .map_err(|err| CliError::ApplyResource(err.to_string()))?;
    info!("\"{}\" with name \"{}\" applied successfully", kind, name);

    Ok(())
}

fn k8s_client(namespace: String) -> Result<SyncK8sClient, CliError> {
    debug!("Starting the runtime");
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Tokio should be able to create a runtime"),
    );

    debug!("Starting the k8s client");
    Ok(SyncK8sClient::try_new(runtime, namespace)?)
}
