use std::sync::Arc;

use clap::{Parser, Subcommand};
use helm_release::{create_helm_release, HelmReleaseData};
use helm_repository::{create_helm_repository, HelmRepositoryData};
use newrelic_agent_control::{
    http::tls::install_rustls_default_crypto_provider, k8s::client::SyncK8sClient,
};
use tracing::{debug, info, Level};

mod helm_release;
mod helm_repository;
mod utils;

/// Manage Helm releases and repositories in Kubernetes
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
        resource_type: CommandResourceType,
    },
}

#[derive(Debug, Subcommand )]
enum CommandResourceType {
    /// Operate over a helm release object
    HelmRelease(HelmReleaseData),

    /// Operate over a helm repository object
    HelmRepository(HelmRepositoryData),
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
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
            .build()?,
    );

    debug!("Starting the k8s client");
    let k8s_client = Arc::new(SyncK8sClient::try_new(runtime, cli.namespace)?);

    match cli.operation {
        Operations::Create { resource_type } => match resource_type {
            CommandResourceType::HelmRepository(helm_repository_data) => {
                create_helm_repository(k8s_client.clone(), helm_repository_data)
            }
            CommandResourceType::HelmRelease(helm_release_data) => {
                create_helm_release(k8s_client.clone(), helm_release_data)
            }
        },
    };

    debug!("Operation finished");

    Ok(())
}
