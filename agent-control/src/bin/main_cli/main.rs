use std::{process::ExitCode, sync::Arc};

use clap::{Parser, Subcommand};
use errors::{CliError, ParseError};
use helm_release::HelmReleaseData;
use helm_repository::HelmRepositoryData;
use kube::api::DynamicObject;
use newrelic_agent_control::{
    http::tls::install_rustls_default_crypto_provider, k8s::client::SyncK8sClient,
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
    log_level: Option<Level>,
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

impl ResourceType {
    fn as_trait_object(&self) -> &dyn ResourceTypeHandler {
        match self {
            ResourceType::HelmRelease(data) => data,
            ResourceType::HelmRepository(data) => data,
        }
    }
}

trait ResourceTypeHandler {
    fn type_name(&self) -> String;
    fn to_dynamic_object(&self, namespace: String) -> Result<DynamicObject, ParseError>;
}

fn main() -> ExitCode {
    debug!("Starting cli");
    let cli = Cli::parse();
    debug!("Arguments parsed: {:?}", cli);

    debug!("Setting up logging with level: {:?}", cli.log_level);
    tracing_subscriber::fmt::fmt()
        .with_max_level(cli.log_level)
        .init();

    debug!("Installing default rustls crypto provider");
    install_rustls_default_crypto_provider();

    debug!("Starting the runtime");
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Tokio should be able to create a runtime"),
    );

    debug!("Starting the k8s client");
    let k8s_client = Arc::new(SyncK8sClient::try_new(runtime, cli.namespace).unwrap());

    let result = match cli.operation {
        Operations::Create { resource_type } => apply_resource(k8s_client.clone(), resource_type),
    };

    match result {
        Ok(_) => ExitCode::SUCCESS,
        Err(err) => {
            error!("Operation failed: {:?}", err);
            err.to_exit_code()
        }
    }
}

fn apply_resource(
    k8s_client: Arc<SyncK8sClient>,
    resource_type: ResourceType,
) -> Result<(), CliError> {
    let resource_type_handler = resource_type.as_trait_object();

    info!("Creating {}", resource_type_handler.type_name());
    let dynamic_object =
        resource_type_handler.to_dynamic_object(k8s_client.default_namespace().to_string())?;
    k8s_client
        .apply_dynamic_object(&dynamic_object)
        .map_err(|err| CliError::ApplyResource(err.to_string()))?;
    info!("{} created", resource_type_handler.type_name());

    Ok(())
}
