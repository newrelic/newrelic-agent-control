use std::collections::BTreeMap;

use kube::{
    api::{DynamicObject, ObjectMeta, TypeMeta},
    core::Duration,
};
use tracing::info;

use crate::errors::ParseError;

pub struct HelmRepositoryData {
    /// Object name
    pub name: String,

    /// Repository url
    pub url: String,

    /// Identifying metadata
    ///
    /// Labels are used to select and find collection of objects.
    pub labels: Option<BTreeMap<String, String>>,

    /// Non-identifying metadata
    pub annotations: Option<BTreeMap<String, String>>,

    /// Interval at which the repository will be fetched again
    ///
    /// The controller will fetch the Helm repository
    /// index yaml at the specified interval.
    ///
    /// The interval must be in the [Go duration format](https://pkg.go.dev/time#ParseDuration).
    pub interval: Duration,
}

impl TryFrom<HelmRepositoryData> for DynamicObject {
    type Error = ParseError;

    fn try_from(value: HelmRepositoryData) -> Result<Self, Self::Error> {
        info!(
            "Creating Helm repository representation with name \"{}\"",
            value.name
        );

        let dynamic_object = DynamicObject {
            types: Some(helmrepository_type_meta()),
            metadata: ObjectMeta {
                name: Some(value.name.clone()),
                labels: value.labels,
                annotations: value.annotations,
                ..Default::default()
            },
            data: serde_json::json!({
                "spec": {
                    "url": value.url,
                    "interval": value.interval,
                }
            }),
        };
        info!(
            "Helm repository representation with name \"{}\" created",
            value.name
        );

        Ok(dynamic_object)
    }
}

pub fn helmrepository_type_meta() -> TypeMeta {
    TypeMeta {
        api_version: "source.toolkit.fluxcd.io/v1".to_string(),
        kind: "HelmRepository".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::utils::parse_key_value_pairs;

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
            labels: Some(parse_key_value_pairs("label1=value1,label2=value2").unwrap()),
            annotations: Some(
                parse_key_value_pairs("annotation1=value1,annotation2=value2").unwrap(),
            ),
            interval: Duration::from_str("6m").unwrap(),
        };
        let actual_dynamic_object = DynamicObject::try_from(helm_repository_data).unwrap();

        assert_eq!(actual_dynamic_object, expected_dynamic_object);
    }
}
