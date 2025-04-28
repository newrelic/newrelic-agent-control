use clap::{Parser, Subcommand};
use kube::api::{DynamicObject, ObjectMeta, TypeMeta};
use newrelic_agent_control::{
    agent_control::config::helmrelease_v2_type_meta,
    http::tls::install_rustls_default_crypto_provider, k8s::client::SyncK8sClient,
};
use tracing::{debug, info, Level};

use std::{fs, path::PathBuf, sync::Arc};

#[derive(Parser)]
#[command(about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    operation: Operations,

    #[arg(short, long, global = true, default_value = "default")]
    namespace: String,

    #[arg(long, global = true, default_value = "info")]
    log_level: Option<Level>,
}

#[derive(Subcommand)]
enum Operations {
    Create {
        #[command(subcommand)]
        resource_type: CommandResourceType,
    },
}

#[derive(Subcommand)]
enum CommandResourceType {
    HelmRelease(HelmReleaseData),
    HelmRepository(HelmRepositoryData),
}

#[derive(Parser)]
struct HelmRepositoryData {
    #[arg(long)]
    name: String,

    #[arg(long)]
    url: String,

    #[arg(long, default_value = "24h")]
    interval: String,
}

#[derive(Parser)]
struct HelmReleaseData {
    #[arg(long)]
    name: String,

    #[arg(long)]
    chart_name: String,

    #[arg(long)]
    chart_version: String,

    #[arg(long)]
    repository_name: String,

    #[arg(long)]
    values: Option<String>,

    #[arg(long)]
    values_file: Option<PathBuf>,

    #[arg(long, default_value = "24h")]
    interval: String,

    #[arg(long, default_value = "24h")]
    timeout: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    info!("Starting the cli");
    let cli = Cli::parse();

    // TODO: Make configurable through cli options
    tracing_subscriber::fmt::fmt()
        .with_max_level(cli.log_level)
        .init();

    debug!("Log level set");

    info!("Starting k8s installation job...");
    install_rustls_default_crypto_provider();

    info!("Starting the runtime");
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?,
    );
    info!("Starting the k8s client");
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

    info!("K8s installation job completed.");

    Ok(())
}

fn create_helm_repository(
    k8s_client: Arc<SyncK8sClient>,
    helm_repository_data: HelmRepositoryData,
) {
    info!("Creating Helm repository");
    let helm_repo = DynamicObject {
        types: Some(TypeMeta {
            api_version: "source.toolkit.fluxcd.io/v1".to_string(),
            kind: "HelmRepository".to_string(),
        }),
        metadata: ObjectMeta {
            name: Some(helm_repository_data.name),
            namespace: Some(k8s_client.default_namespace().to_string()),
            ..Default::default()
        },
        data: serde_json::json!({
            "spec": {
                "url": helm_repository_data.url,
                "interval": helm_repository_data.interval,
            }
        }),
    };

    info!("Applying Helm repository");
    k8s_client.apply_dynamic_object(&helm_repo).unwrap();
    info!("Helm repository applied.");
}

fn create_helm_release(k8s_client: Arc<SyncK8sClient>, helm_release_data: HelmReleaseData) {
    info!("Creating Helm release");

    let mut data = serde_json::json!({
        "spec": {
            "interval": helm_release_data.interval,
            "timeout": helm_release_data.timeout,
            "chart": {
                "spec": {
                    "chart": helm_release_data.chart_name,
                    "version": helm_release_data.chart_version,
                    "sourceRef": {
                        "kind": "HelmRepository",
                        "name": helm_release_data.repository_name,
                    },
                    "interval": helm_release_data.interval,
                },
            }
        }
    });

    if let Some(values) = parse_helm_release_values(&helm_release_data) {
        data["spec"]["values"] = values;
    }

    let helm_release = DynamicObject {
        types: Some(helmrelease_v2_type_meta()),
        metadata: ObjectMeta {
            name: Some(helm_release_data.name.clone()),
            namespace: Some(k8s_client.default_namespace().to_string()),
            ..Default::default()
        },
        data,
    };
    info!("Helm release object created: {:?}", helm_release);

    info!("Applying helm release");
    k8s_client.apply_dynamic_object(&helm_release).unwrap();
    info!("Helm release applied.");
}

fn parse_helm_release_values(helm_release_data: &HelmReleaseData) -> Option<serde_json::Value> {
    let values = &helm_release_data.values;
    let values_file = &helm_release_data.values_file;
    match (values, values_file) {
        (Some(_), Some(_)) => {
            panic!("You can only specify one of --values or --values-file");
        }
        (Some(values), None) => {
            let values = serde_yaml::from_str(values).unwrap();
            Some(serde_json::from_value(values).unwrap())
        }
        (None, Some(values_file)) => {
            let values = fs::read_to_string(values_file).unwrap();
            let values = serde_yaml::from_str(&values).unwrap();
            Some(serde_json::from_value(values).unwrap())
        }
        (None, None) => None,
    }
}
