use clap::Parser;
use kube::{
    api::{DynamicObject, ObjectMeta, TypeMeta},
    core::Duration,
};
use tracing::debug;

use crate::{errors::ParseError, utils::parse_key_value_pairs};

pub const TYPE_NAME: &str = "Helm Repository";

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
    pub interval: Duration,
}

impl TryFrom<HelmRepositoryData> for DynamicObject {
    type Error = ParseError;

    fn try_from(value: HelmRepositoryData) -> Result<Self, Self::Error> {
        debug!("Creating Helm repository object representation");

        let labels = parse_key_value_pairs(value.labels.as_deref().unwrap_or_default());
        debug!("Parsed labels: {:?}", labels);

        let annotations = parse_key_value_pairs(value.annotations.as_deref().unwrap_or_default());
        debug!("Parsed annotations: {:?}", annotations);

        let dynamic_object = DynamicObject {
            types: Some(TypeMeta {
                api_version: "source.toolkit.fluxcd.io/v1".to_string(),
                kind: "HelmRepository".to_string(),
            }),
            metadata: ObjectMeta {
                name: Some(value.name.clone()),
                labels,
                annotations,
                ..Default::default()
            },
            data: serde_json::json!({
                "spec": {
                    "url": value.url,
                    "interval": value.interval,
                }
            }),
        };
        debug!("Helm repository object representation created");

        Ok(dynamic_object)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

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
                    "interval": "360s",
                }
            }),
        };

        let helm_repository_data = HelmRepositoryData {
            name: "test-repository".to_string(),
            url: "https://example.com/helm-charts".to_string(),
            labels: Some("label1=value1,label2=value2".to_string()),
            annotations: Some("annotation1=value1,annotation2=value2".to_string()),
            interval: Duration::from_str("6m").unwrap(),
        };
        let actual_dynamic_object = DynamicObject::try_from(helm_repository_data).unwrap();

        assert_eq!(actual_dynamic_object, expected_dynamic_object);
    }
}
