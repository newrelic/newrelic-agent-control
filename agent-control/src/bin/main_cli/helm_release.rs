use std::fs;

use clap::Parser;
use kube::api::{DynamicObject, ObjectMeta};
use newrelic_agent_control::agent_control::config::helmrelease_v2_type_meta;
use tracing::{debug, info};

use crate::{errors::ParseError, utils::parse_key_value_pairs, ToDynamicObject};

pub const TYPE_NAME: &str = "Helm release";
const FILE_PREFIX: &str = "fs://";

#[derive(Debug, Parser)]
pub struct HelmReleaseData {
    /// Object name
    #[arg(long)]
    pub name: String,

    /// Name of the chart to deploy
    #[arg(long)]
    pub chart_name: String,

    /// Version of the chart to deploy
    #[arg(long)]
    pub chart_version: String,

    /// Name of the Helm Repository from where to get the chart
    ///
    /// The Helm Repository must already be created in the
    /// cluster.
    #[arg(long)]
    pub repository_name: String,

    /// Chart values
    ///
    /// A yaml file or yaml string with the values of the chart.
    /// If the value starts with `fs://`, it is treated as a
    /// file path. Otherwise, it is treated as a string.
    #[arg(long)]
    pub values: Option<String>,

    /// Identifying metadata
    ///
    /// Labels are used to select and find collection of objects.
    #[arg(long)]
    pub labels: Option<String>,

    /// Non-identifying metadata
    #[arg(long)]
    pub annotations: Option<String>,

    /// Interval at which the release is reconciled
    ///
    /// The controller will check the Helm release is in
    /// the desired state at the specified interval.
    ///
    /// The interval must be in the [Go duration format](https://pkg.go.dev/time#ParseDuration).
    #[arg(long, default_value = "5m")]
    pub interval: String,

    /// Timeout for some Helm actions
    ///
    /// Some Helm actions like install, upgrade or rollback
    /// will timeout at the specified elapsed time.
    ///
    /// The timeout must be in the [Go duration format](https://pkg.go.dev/time#ParseDuration).
    #[arg(long, default_value = "5m")]
    pub timeout: String,
}

impl ToDynamicObject for HelmReleaseData {
    fn to_dynamic_object(&self, namespace: String) -> Result<DynamicObject, ParseError> {
        info!("Creating Helm release object representation");

        let mut data = serde_json::json!({
            "spec": {
                "interval": self.interval,
                "timeout": self.timeout,
                "chart": {
                    "spec": {
                        "chart": self.chart_name,
                        "version": self.chart_version,
                        "sourceRef": {
                            "kind": "HelmRepository",
                            "name": self.repository_name,
                        },
                        "interval": self.interval,
                    },
                }
            }
        });

        if let Some(values) = self.parse_values()? {
            debug!("Parsed values: {:?}", values);
            data["spec"]["values"] = values;
        }

        let labels = parse_key_value_pairs(self.labels.as_deref().unwrap_or_default());
        debug!("Parsed labels: {:?}", labels);

        let annotations = parse_key_value_pairs(self.annotations.as_deref().unwrap_or_default());
        debug!("Parsed annotations: {:?}", annotations);

        let dynamic_object = DynamicObject {
            types: Some(helmrelease_v2_type_meta()),
            metadata: ObjectMeta {
                name: Some(self.name.clone()),
                namespace: Some(namespace),
                labels,
                annotations,
                ..Default::default()
            },
            data,
        };
        debug!("Helm release object representation created");

        Ok(dynamic_object)
    }
}

impl HelmReleaseData {
    fn parse_values(&self) -> Result<Option<serde_json::Value>, ParseError> {
        let Some(input) = &self.values else {
            return Ok(None);
        };

        let values = match input.strip_prefix(FILE_PREFIX) {
            Some(path) => &fs::read_to_string(path)?,
            None => input,
        };
        let yaml_values = serde_yaml::from_str(values)?;
        let json_values =
            serde_json::from_value(yaml_values).expect("serde_yaml should return a valid `Value`");

        Ok(Some(json_values))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use tempfile::NamedTempFile;

    fn helm_release_data() -> HelmReleaseData {
        HelmReleaseData {
            name: "test-release".to_string(),
            chart_name: "test-chart".to_string(),
            chart_version: "1.0.0".to_string(),
            repository_name: "test-repository".to_string(),
            values: Some("value1: value1\nvalue2: value2".to_string()),
            labels: Some("label1=value1,label2=value2".to_string()),
            annotations: Some("annotation1=value1,annotation2=value2".to_string()),
            interval: "6m".to_string(),
            timeout: "5m".to_string(),
        }
    }

    fn helm_release_dynamic_object() -> DynamicObject {
        DynamicObject {
            types: Some(helmrelease_v2_type_meta()),
            metadata: ObjectMeta {
                name: Some("test-release".to_string()),
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
                    "interval": "6m",
                    "timeout": "5m",
                    "chart": {
                        "spec": {
                            "chart": "test-chart",
                            "version": "1.0.0",
                            "sourceRef": {
                                "kind": "HelmRepository",
                                "name": "test-repository",
                            },
                            "interval": "6m",
                        },
                    },
                    "values": {
                        "value1": "value1",
                        "value2": "value2",
                    },
                },
            }),
        }
    }

    #[test]
    fn test_to_dynamic_object() {
        assert_eq!(
            helm_release_data()
                .to_dynamic_object("test-namespace".to_string())
                .unwrap(),
            helm_release_dynamic_object()
        );
    }

    #[test]
    fn test_parse_values() {
        assert_eq!(
            helm_release_data().parse_values().unwrap(),
            Some(serde_json::json!({
                "value1": "value1",
                "value2": "value2"
            }))
        );
    }

    #[test]
    fn test_parse_values_no_values() {
        let mut helm_release_data = helm_release_data();
        helm_release_data.values = None;

        assert_eq!(helm_release_data.parse_values().unwrap(), None);
    }

    #[test]
    fn test_parse_values_from_string() {
        let mut helm_release_data = helm_release_data();
        helm_release_data.values =
            Some("{outer: {inner1: 'value1', inner2: 'value2'}}".to_string());

        assert_eq!(
            helm_release_data.parse_values().unwrap(),
            Some(serde_json::json!({
            "outer": {
                "inner1": "value1",
                "inner2": "value2"
            }}))
        );
    }

    #[test]
    fn test_parse_values_from_string_throws_error_invalid_yaml() {
        let mut helm_release_data = helm_release_data();
        helm_release_data.values = Some("key1: value1\nkey2 value2".to_string());

        assert!(helm_release_data.parse_values().is_err());
    }

    #[test]
    fn test_parse_values_from_file() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let _ = temp_file
            .write(b"{outer: {inner1: 'value1', inner2: 'value2'}}")
            .unwrap();

        let mut helm_release_data = helm_release_data();
        helm_release_data.values = Some(format!("fs://{}", temp_file.path().display()));

        assert_eq!(
            helm_release_data.parse_values().unwrap(),
            Some(serde_json::json!({
            "outer": {
                "inner1": "value1",
                "inner2": "value2"
            }}))
        );
    }

    #[test]
    fn test_parse_values_from_file_throws_error_invalid_yaml() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let _ = temp_file.write(b"key1: value1\nkey2 value2").unwrap();

        let mut helm_release_data = helm_release_data();
        helm_release_data.values = Some(format!("fs://{}", temp_file.path().display()));

        assert!(helm_release_data.parse_values().is_err());
    }
}
