use std::sync::Arc;

use clap::Parser;
use kube::api::{DynamicObject, ObjectMeta, TypeMeta};
use newrelic_agent_control::k8s::client::SyncK8sClient;
use tracing::{debug, info};

use crate::utils::parse_key_value_pairs;

#[derive(Debug, Parser)]
pub struct HelmRepositoryData {
    /// Object name
    #[arg(long)]
    pub name: String,

    /// Repository url
    #[arg(long)]
    pub url: String,

    /// Identifying metadata
    ///
    /// Labels are used to select and find collection of objects.
    #[arg(long)]
    pub labels: Option<String>,

    /// Non-identifying metadata
    #[arg(long)]
    pub annotations: Option<String>,

    /// Interval at which the repository will be fetched again
    ///
    /// The controller will fetch the Helm repository
    /// index yaml at the specified interval.
    ///
    /// The interval must be in the [Go duration format](https://pkg.go.dev/time#ParseDuration).
    #[arg(long, default_value = "5m")]
    pub interval: String,
}

pub fn create_helm_repository(
    k8s_client: Arc<SyncK8sClient>,
    helm_repository_data: HelmRepositoryData,
) {
    info!("Creating Helm repository");

    let labels = parse_key_value_pairs(&helm_repository_data.labels.unwrap_or_default());
    debug!("Parsed labels: {:?}", labels);

    let annotations = parse_key_value_pairs(&helm_repository_data.annotations.unwrap_or_default());
    debug!("Parsed annotations: {:?}", annotations);

    let helm_repo = DynamicObject {
        types: Some(TypeMeta {
            api_version: "source.toolkit.fluxcd.io/v1".to_string(),
            kind: "HelmRepository".to_string(),
        }),
        metadata: ObjectMeta {
            name: Some(helm_repository_data.name),
            namespace: Some(k8s_client.default_namespace().to_string()),
            labels,
            annotations,
            ..Default::default()
        },
        data: serde_json::json!({
            "spec": {
                "url": helm_repository_data.url,
                "interval": helm_repository_data.interval,
            }
        }),
    };
    info!("Helm repository object representation created");

    info!("Applying Helm repository");
    k8s_client.apply_dynamic_object(&helm_repo).unwrap();
    info!("Helm repository applied.");
}
