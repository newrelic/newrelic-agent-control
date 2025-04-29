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

impl HelmRepositoryData {
    fn to_dynamic_object(&self, namespace: String) -> DynamicObject {
        debug!("Creating Helm repository object representation");

        let labels = parse_key_value_pairs(self.labels.as_deref().unwrap_or_default());
        debug!("Parsed labels: {:?}", labels);

        let annotations = parse_key_value_pairs(self.annotations.as_deref().unwrap_or_default());
        debug!("Parsed annotations: {:?}", annotations);

        let dynamic_object = DynamicObject {
            types: Some(TypeMeta {
                api_version: "source.toolkit.fluxcd.io/v1".to_string(),
                kind: "HelmRepository".to_string(),
            }),
            metadata: ObjectMeta {
                name: Some(self.name.clone()),
                namespace: Some(namespace),
                labels,
                annotations,
                ..Default::default()
            },
            data: serde_json::json!({
                "spec": {
                    "url": self.url,
                    "interval": self.interval,
                }
            }),
        };
        debug!("Helm repository object representation created");

        dynamic_object
    }
}

pub fn create_helm_repository(
    k8s_client: Arc<SyncK8sClient>,
    helm_repository_data: HelmRepositoryData,
) {
    info!("Creating Helm repository");
    let helm_repository =
        helm_repository_data.to_dynamic_object(k8s_client.default_namespace().to_string());
    k8s_client.apply_dynamic_object(&helm_repository).unwrap();
    info!("Helm repository created");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_dynamic_object() {
        let expected_dynamic_object = DynamicObject {
            types: Some(TypeMeta {
                api_version: "source.toolkit.fluxcd.io/v1".to_string(),
                kind: "HelmRepository".to_string(),
            }),
            metadata: ObjectMeta {
                name: Some("test-repository".to_string()),
                namespace: Some("test-namespace".to_string()),
                labels: Some(
                    vec![
                        ("label1".to_string(), "value1".to_string()),
                        ("label2".to_string(), "value2".to_string()),
                    ]
                    .into_iter()
                    .collect(),
                ),
                annotations: Some(
                    vec![
                        ("annotation1".to_string(), "value1".to_string()),
                        ("annotation2".to_string(), "value2".to_string()),
                    ]
                    .into_iter()
                    .collect(),
                ),
                ..Default::default()
            },
            data: serde_json::json!({
                "spec": {
                    "url": "https://example.com/helm-charts",
                    "interval": "6m",
                }
            }),
        };

        let helm_repository_data = HelmRepositoryData {
            name: "test-repository".to_string(),
            url: "https://example.com/helm-charts".to_string(),
            labels: Some("label1=value1,label2=value2".to_string()),
            annotations: Some("annotation1=value1,annotation2=value2".to_string()),
            interval: "6m".to_string(),
        };
        let actual_dynamic_object =
            helm_repository_data.to_dynamic_object("test-namespace".to_string());

        assert_eq!(actual_dynamic_object, expected_dynamic_object);
    }
}
