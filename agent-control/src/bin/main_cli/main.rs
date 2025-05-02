use std::{path::PathBuf, process::ExitCode, sync::Arc};

use clap::{Parser, Subcommand};
use errors::{CliError, ParseError};
use helm_release::{HelmReleaseData, TYPE_NAME as HELM_RELEASE_TYPE_NAME};
use helm_repository::{HelmRepositoryData, TYPE_NAME as HELM_REPOSITORY_TYPE_NAME};
use kube::api::DynamicObject;
use newrelic_agent_control::{
    agent_control::defaults::AGENT_CONTROL_LOG_DIR,
    http::tls::install_rustls_default_crypto_provider,
    instrumentation::{
        config::logs::config::LoggingConfig,
        tracing::{try_init_tracing, TracingConfig},
    },
    k8s::client::SyncK8sClient,
};
use tracing::{debug, error, info, Level};

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

trait ToDynamicObject {
    fn to_dynamic_object(&self, namespace: String) -> Result<DynamicObject, ParseError>;
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
            ResourceType::HelmRelease(data) => {
                apply_resource(cli.namespace, data, HELM_RELEASE_TYPE_NAME)
            }
            ResourceType::HelmRepository(data) => {
                apply_resource(cli.namespace, data, HELM_REPOSITORY_TYPE_NAME)
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

fn apply_resource<T: ToDynamicObject>(
    namespace: String,
    data: T,
    type_name: &str,
) -> Result<(), CliError> {
    info!("Creating {}", type_name);
    let dynamic_object = data.to_dynamic_object(namespace.clone())?;
    let k8s_client = k8s_client(namespace.clone())?;
    k8s_client
        .apply_dynamic_object(&dynamic_object)
        .map_err(|err| CliError::ApplyResource(err.to_string()))?;
    info!("{} created", type_name);

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
