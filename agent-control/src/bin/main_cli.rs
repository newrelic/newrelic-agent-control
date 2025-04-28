use clap::{Parser, Subcommand};
use kube::api::{DynamicObject, ObjectMeta, TypeMeta};
use newrelic_agent_control::{
    agent_control::config::helmrelease_v2_type_meta,
    http::tls::install_rustls_default_crypto_provider, k8s::client::SyncK8sClient,
};
use tracing::{info, Level};

use std::{collections::BTreeMap, fs, path::PathBuf, sync::Arc};

/// Manage Helm releases and repositories in Kubernetes
#[derive(Parser)]
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

#[derive(Subcommand)]
enum Operations {
    /// Create an object in the cluster
    Create {
        #[command(subcommand)]
        resource_type: CommandResourceType,
    },
}

#[derive(Subcommand)]
enum CommandResourceType {
    /// Operate over a helm release object
    HelmRelease(HelmReleaseData),

    /// Operate over a helm repository object
    HelmRepository(HelmRepositoryData),
}

#[derive(Parser)]
struct HelmRepositoryData {
    /// Object name
    #[arg(long)]
    name: String,

    /// Repository url
    #[arg(long)]
    url: String,

    /// Identifying metadata
    ///
    /// Labels are used to select and find collection of objects.
    #[arg(long)]
    labels: Option<String>,

    /// Non-identifying metadata
    #[arg(long)]
    annotations: Option<String>,

    /// Interval at which the repository will be fetched again
    ///
    /// The controller will fetch the Helm repository
    /// index yaml at the specified interval.
    ///
    /// The interval must be in the [Go duration format](https://pkg.go.dev/time#ParseDuration).
    #[arg(long, default_value = "5m")]
    interval: String,
}

#[derive(Parser)]
struct HelmReleaseData {
    /// Object name
    #[arg(long)]
    name: String,

    /// Name of the chart to deploy
    #[arg(long)]
    chart_name: String,

    /// Version of the chart to deploy
    #[arg(long)]
    chart_version: String,

    /// Name of the Helm Repository from where to get the chart
    ///
    /// The Helm Repository must already be created in the
    /// cluster.
    #[arg(long)]
    repository_name: String,

    /// Chart values as string
    ///
    /// The values of the chart as a yaml string.
    /// The values can also be read from a file using `--values-file`,
    /// but only one flag can be used at once.
    #[arg(long)]
    values: Option<String>,

    /// Chart values file
    ///
    /// A yaml file with the values of the chart.
    /// The values can also be passed as a string with `--values`,
    /// but only one flag can be used at once.
    #[arg(long)]
    values_file: Option<PathBuf>,

    /// Identifying metadata
    ///
    /// Labels are used to select and find collection of objects.
    #[arg(long)]
    labels: Option<String>,

    /// Non-identifying metadata
    #[arg(long)]
    annotations: Option<String>,

    /// Interval at which the release is reconciled
    ///
    /// The controller will check the Helm release is in
    /// the desired state at the specified interval.
    ///
    /// The interval must be in the [Go duration format](https://pkg.go.dev/time#ParseDuration).
    #[arg(long, default_value = "5m")]
    interval: String,

    /// Timeout for some Helm actions
    ///
    /// Some Helm actions like install, upgrade or rollback
    /// will timeout at the specified elapsed time.
    ///
    /// The timeout must be in the [Go duration format](https://pkg.go.dev/time#ParseDuration).
    #[arg(long, default_value = "5m")]
    timeout: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    info!("Starting the cli");
    let cli = Cli::parse();

    tracing_subscriber::fmt::fmt()
        .with_max_level(cli.log_level)
        .init();

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
            annotations: parse_key_value_pairs(
                &helm_repository_data.annotations.unwrap_or_default(),
            ),
            labels: parse_key_value_pairs(&helm_repository_data.labels.unwrap_or_default()),
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

fn parse_key_value_pairs(data: &str) -> Option<BTreeMap<String, String>> {
    let mut parsed_key_values = BTreeMap::new();

    let pairs = data.split(',');
    let key_values = pairs.map(|pair| pair.split_once('='));
    let valid_key_values = key_values.flatten();
    valid_key_values.for_each(|(key, value)| {
        parsed_key_values.insert(key.trim().to_string(), value.trim().to_string());
    });

    match parsed_key_values.is_empty() {
        true => None,
        false => Some(parsed_key_values),
    }
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
            annotations: parse_key_value_pairs(&helm_release_data.annotations.unwrap_or_default()),
            labels: parse_key_value_pairs(&helm_release_data.labels.unwrap_or_default()),
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
    match (&helm_release_data.values, &helm_release_data.values_file) {
        (Some(_), Some(_)) => {
            panic!("You can only specify one of --values or --values-file");
        }
        (Some(values), None) => {
            let values = serde_yaml::from_str(&values).unwrap();
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
