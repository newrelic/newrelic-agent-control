use std::collections::BTreeMap;

use kube::{
    api::{DynamicObject, ObjectMeta},
    core::Duration,
};
use newrelic_agent_control::agent_control::config::helmrelease_v2_type_meta;
use tracing::{debug, info};

use crate::errors::ParseError;

pub struct HelmReleaseData {
    /// Object name
    pub name: String,

    /// Name of the chart to deploy
    pub chart_name: String,

    /// Version of the chart to deploy
    pub chart_version: String,

    /// Name of the Helm Repository from where to get the chart
    ///
    /// The Helm Repository must already be created in the
    /// cluster.
    pub repository_name: String,

    /// Chart values
    pub values: Option<serde_json::Value>,

    /// Secret name from where to get the chart values
    pub values_from_secret: Option<String>,

    /// Identifying metadata
    ///
    /// Labels are used to select and find collection of objects.
    pub labels: Option<BTreeMap<String, String>>,

    /// Non-identifying metadata
    pub annotations: Option<BTreeMap<String, String>>,

    /// Interval at which the release is reconciled
    ///
    /// The controller will check the Helm release is in
    /// the desired state at the specified interval.
    ///
    /// The interval must be in the [Go duration format](https://pkg.go.dev/time#ParseDuration).
    pub interval: Duration,

    /// Timeout for some Helm actions
    ///
    /// Some Helm actions like install, upgrade or rollback
    /// will timeout at the specified elapsed time.
    ///
    /// The timeout must be in the [Go duration format](https://pkg.go.dev/time#ParseDuration).
    pub timeout: Duration,
}

impl TryFrom<HelmReleaseData> for DynamicObject {
    type Error = ParseError;

    fn try_from(value: HelmReleaseData) -> Result<Self, Self::Error> {
        info!(
            "Creating Helm release representation with name \"{}\"",
            value.name
        );

        let mut data = serde_json::json!({
            "spec": {
                "interval": value.interval,
                "timeout": value.timeout,
                "chart": {
                    "spec": {
                        "chart": value.chart_name,
                        "version": value.chart_version,
                        "sourceRef": {
                            "kind": "HelmRepository",
                            "name": value.repository_name,
                        },
                        "interval": value.interval,
                    },
                }
            }
        });

        if let Some(values) = value.values {
            data["spec"]["values"] = values;
        }

        if let Some(values_from_secret) = value.values_from_secret {
            debug!("Parsed values from secret: {:?}", values_from_secret);
            data["spec"]["valuesFrom"] = serde_json::json!([{
                "kind": "Secret",
                "name": values_from_secret,
                "valuesKey": "values.yaml",
            }]);
        }

        let dynamic_object = DynamicObject {
            types: Some(helmrelease_v2_type_meta()),
            metadata: ObjectMeta {
                name: Some(value.name.clone()),
                labels: value.labels,
                annotations: value.annotations,
                ..Default::default()
            },
            data,
        };
        info!(
            "Helm release representation with name \"{}\" created",
            value.name
        );

        Ok(dynamic_object)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn test_to_dynamic_object() {
        let helm_release_data = HelmReleaseData {
            name: "test-release".to_string(),
            chart_name: "test-chart".to_string(),
            chart_version: "1.0.0".to_string(),
            repository_name: "test-repository".to_string(),
            values: Some(serde_json::json!({
                "value1": "value1",
                "value2": "value2"
            })),
            values_from_secret: Some("test-secret".to_string()),
            labels: Some(BTreeMap::from([
                ("label1".to_string(), "value1".to_string()),
                ("label2".to_string(), "value2".to_string()),
            ])),
            annotations: Some(BTreeMap::from([
                ("annotation1".to_string(), "value1".to_string()),
                ("annotation2".to_string(), "value2".to_string()),
            ])),
            interval: Duration::from_str("6m").unwrap(),
            timeout: Duration::from_str("7m").unwrap(),
        };

        let expected_dynamic_object = DynamicObject {
            types: Some(helmrelease_v2_type_meta()),
            metadata: ObjectMeta {
                name: Some("test-release".to_string()),
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
                    "interval": "360s",
                    "timeout": "420s",
                    "chart": {
                        "spec": {
                            "chart": "test-chart",
                            "version": "1.0.0",
                            "sourceRef": {
                                "kind": "HelmRepository",
                                "name": "test-repository",
                            },
                            "interval": "360s",
                        },
                    },
                    "values": {
                        "value1": "value1",
                        "value2": "value2",
                    },
                    "valuesFrom": [{
                        "kind": "Secret",
                        "name": "test-secret",
                        "valuesKey": "values.yaml",
                    }],
                },
            }),
        };

        let actual_dynamic_object = DynamicObject::try_from(helm_release_data).unwrap();
        assert_eq!(actual_dynamic_object, expected_dynamic_object);
    }
}
