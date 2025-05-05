use std::{path::PathBuf, process::ExitCode, sync::Arc};

use clap::{Parser, Subcommand};
use errors::{CliError, ParseError};
use helm_release::HelmReleaseData;
use helm_repository::HelmRepositoryData;
use kube::api::DynamicObject;
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

mod errors;
mod helm_release;
mod helm_repository;
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
    /// Create an object in the cluster
    Create {
        #[command(subcommand)]
        resource_type: ResourceType,
    },
}

#[derive(Debug, Subcommand)]
enum ResourceType {
    /// Operate over a helm release object
    HelmRelease(HelmReleaseData),

    /// Operate over a helm repository object
    HelmRepository(HelmRepositoryData),
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let logging_config: LoggingConfig =
        serde_yaml::from_str(&format!("level: {}", cli.log_level)).unwrap();
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
        Operations::Create { resource_type } => match resource_type {
            ResourceType::HelmRelease(data) => apply_resource(data, cli.namespace),
            ResourceType::HelmRepository(data) => apply_resource(data, cli.namespace),
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

fn apply_resource<T>(data: T, namespace: String) -> Result<(), CliError>
where
    DynamicObject: TryFrom<T, Error = ParseError>,
{
    info!("Applying resource");
    let dynamic_object = DynamicObject::try_from(data)?;
    let k8s_client = k8s_client(namespace.clone())?;
    k8s_client
        .apply_dynamic_object(&dynamic_object)
        .map_err(|err| CliError::ApplyResource(err.to_string()))?;
    info!("Resource applied successfully");

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
