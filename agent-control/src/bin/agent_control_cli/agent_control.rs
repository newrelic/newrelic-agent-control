use std::{collections::BTreeMap, str::FromStr};

use clap::Parser;
use k8s_openapi::api::core::v1::Secret;
use kube::{
    api::{DynamicObject, ObjectMeta},
    core::Duration,
};
use tracing::{debug, info};

use crate::{
    errors::ParseError,
    resources::{HelmReleaseData, HelmRepositoryData, SecretData},
    utils::parse_key_value_pairs,
};

const REPOSITORY_NAME: &str = "newrelic";
const REPOSITORY_URL: &str = "https://helm-charts.newrelic.com";
const SECRET_NAME: &str = "agent-control-secret";

#[derive(Debug, Parser)]
pub struct AgentControlData {
    /// Release name
    #[arg(long)]
    pub release_name: String,

    /// Version of the agent control chart
    #[arg(long)]
    pub chart_version: String,

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
    /// They will be applied to every resource created for Agent Control.
    #[arg(long)]
    pub labels: Option<String>,

    /// Non-identifying metadata
    ///
    /// They will be applied to every resource created for Agent Control.
    #[arg(long)]
    pub annotations: Option<String>,
}

impl TryFrom<AgentControlData> for Vec<DynamicObject> {
    type Error = ParseError;

    fn try_from(value: AgentControlData) -> Result<Self, Self::Error> {
        info!("Creating Agent Control resources representations");

        let labels = parse_key_value_pairs(value.labels.as_deref().unwrap_or_default());
        debug!("Parsed labels: {:?}", labels);

        let annotations = parse_key_value_pairs(value.annotations.as_deref().unwrap_or_default());
        debug!("Parsed annotations: {:?}", annotations);

        let helm_repository = HelmRepositoryData {
            name: REPOSITORY_NAME.to_string(),
            url: REPOSITORY_URL.to_string(),
            labels: labels.clone(),
            annotations: annotations.clone(),
            interval: Duration::from_str("5m").expect("Hardcoded value should be correct"),
        };
        let repository_object = DynamicObject::try_from(helm_repository)?;

        let mut secret_object = None;
        let values = value.values.map(parse_values).transpose()?;
        if let Some(values) = &values {
            let secret = SecretData(Secret {
                type_: Some("Opaque".to_string()),
                metadata: ObjectMeta {
                    name: Some(SECRET_NAME.to_string()),
                    labels: labels.clone(),
                    annotations: annotations.clone(),
                    ..Default::default()
                },
                string_data: Some(BTreeMap::from_iter(vec![(
                    "values.yaml".to_string(),
                    values.to_string(),
                )])),
                data: None,
                immutable: None,
            });
            secret_object = Some(DynamicObject::try_from(secret)?);
        }

        let helm_release = HelmReleaseData {
            name: value.release_name,
            chart_name: "agent-control-deployment".to_string(),
            chart_version: value.chart_version,
            repository_name: REPOSITORY_NAME.to_string(),
            values: None,
            values_from_secret: secret_object.clone().and(Some(SECRET_NAME.to_string())),
            labels,
            annotations,
            interval: Duration::from_str("5m").expect("Hardcoded value should be correct"),
            timeout: Duration::from_str("5m").expect("Hardcoded value should be correct"),
        };
        let release_object = DynamicObject::try_from(helm_release)?;

        info!("Agent Control resources representations created");

        let objects = vec![Some(repository_object), secret_object, Some(release_object)];
        Ok(objects.into_iter().flatten().collect())
    }
}

fn parse_values(values: String) -> Result<serde_json::Value, ParseError> {
    let values = match values.strip_prefix("fs://") {
        Some(path) => std::fs::read_to_string(path)?,
        None => values,
    };

    let yaml_values = serde_yaml::from_str(&values)?;
    let json_values =
        serde_json::from_value(yaml_values).expect("serde_yaml should return a valid `Value`");

    Ok(json_values)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use newrelic_agent_control::agent_control::config::helmrelease_v2_type_meta;
    use tempfile::NamedTempFile;

    use crate::resources::{helmrepository_type_meta, secret_type_meta};

    use super::*;

    const RELEASE_NAME: &str = "agent-control-deployment-release";
    const VERSION: &str = "1.0.0";

    fn agent_control_data() -> AgentControlData {
        AgentControlData {
            release_name: RELEASE_NAME.to_string(),
            chart_version: VERSION.to_string(),
            values: None,
            labels: None,
            annotations: None,
        }
    }

    fn repository_object() -> DynamicObject {
        DynamicObject {
            types: Some(helmrepository_type_meta()),
            metadata: ObjectMeta {
                name: Some(REPOSITORY_NAME.to_string()),
                ..Default::default()
            },
            data: serde_json::json!({
                "spec": {
                    "url": REPOSITORY_URL,
                    "interval": "300s",
                }
            }),
        }
    }

    fn secret_object() -> DynamicObject {
        DynamicObject {
            types: Some(secret_type_meta()),
            metadata: ObjectMeta {
                name: Some(SECRET_NAME.to_string()),
                ..Default::default()
            },
            data: serde_json::json!({
                "type": "Opaque",
                "stringData": {
                    "values.yaml": "{\"value1\":\"value1\",\"value2\":\"value2\"}"
                }
            }),
        }
    }

    fn release_object() -> DynamicObject {
        DynamicObject {
            types: Some(helmrelease_v2_type_meta()),
            metadata: ObjectMeta {
                name: Some(RELEASE_NAME.to_string()),
                ..Default::default()
            },
            data: serde_json::json!({
                "spec": {
                    "interval": "300s",
                    "timeout": "300s",
                    "chart": {
                        "spec": {
                            "chart": "agent-control-deployment",
                            "version": VERSION,
                            "sourceRef": {
                                "kind": "HelmRepository",
                                "name": REPOSITORY_NAME,
                            },
                            "interval": "300s",
                        }
                    }
                }
            }),
        }
    }

    #[test]
    fn test_to_dynamic_objects_no_values() {
        let dynamic_objects = Vec::<DynamicObject>::try_from(agent_control_data()).unwrap();
        assert_eq!(dynamic_objects, vec![repository_object(), release_object()]);
    }

    #[test]
    fn test_to_dynamic_objects_with_values() {
        let mut agent_control_data = agent_control_data();
        agent_control_data.values = Some("value1: value1\nvalue2: value2".to_string());
        let dynamic_objects = Vec::<DynamicObject>::try_from(agent_control_data).unwrap();

        let mut expected_release_object = release_object();
        expected_release_object.data["spec"]["valuesFrom"] = serde_json::json!([{
            "kind": "Secret",
            "name": SECRET_NAME,
            "valuesKey": "values.yaml",
        }]);
        assert_eq!(
            dynamic_objects,
            vec![
                repository_object(),
                secret_object(),
                expected_release_object
            ]
        );
    }

    #[test]
    fn test_to_dynamic_objects_with_values_labels_and_annotations() {
        let mut agent_control_data = agent_control_data();
        agent_control_data.values = Some("value1: value1\nvalue2: value2".to_string());
        agent_control_data.labels = Some("label1=value1,label2=value2".to_string());
        agent_control_data.annotations = Some("annotation1=value1,annotation2=value2".to_string());
        let dynamic_objects = Vec::<DynamicObject>::try_from(agent_control_data).unwrap();

        let labels = Some(
            vec![
                ("label1".to_string(), "value1".to_string()),
                ("label2".to_string(), "value2".to_string()),
            ]
            .into_iter()
            .collect(),
        );
        let annotations = Some(
            vec![
                ("annotation1".to_string(), "value1".to_string()),
                ("annotation2".to_string(), "value2".to_string()),
            ]
            .into_iter()
            .collect(),
        );

        let mut expected_repository_object = repository_object();
        expected_repository_object.metadata.labels = labels.clone();
        expected_repository_object.metadata.annotations = annotations.clone();

        let mut expected_secret_object = secret_object();
        expected_secret_object.metadata.labels = labels.clone();
        expected_secret_object.metadata.annotations = annotations.clone();

        let mut expected_release_object = release_object();
        expected_release_object.data["spec"]["valuesFrom"] = serde_json::json!([{
            "kind": "Secret",
            "name": SECRET_NAME,
            "valuesKey": "values.yaml",
        }]);
        expected_release_object.metadata.labels = labels;
        expected_release_object.metadata.annotations = annotations;

        assert_eq!(
            dynamic_objects,
            vec![
                expected_repository_object,
                expected_secret_object,
                expected_release_object
            ]
        );
    }

    #[test]
    fn test_parse_values_from_string() {
        assert_eq!(
            parse_values("value1: value1\nvalue2: value2".to_string()).unwrap(),
            serde_json::json!({
                "value1": "value1",
                "value2": "value2"
            })
        );
    }

    #[test]
    fn test_parse_values_from_string_throws_error_invalid_yaml() {
        assert!(parse_values("key1: value1\nkey2 value2".to_string()).is_err());
    }

    #[test]
    fn test_parse_values_from_file() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let _ = temp_file
            .write(b"{outer: {inner1: 'value1', inner2: 'value2'}}")
            .unwrap();
        assert_eq!(
            parse_values(format!("fs://{}", temp_file.path().display())).unwrap(),
            serde_json::json!({
            "outer": {
                "inner1": "value1",
                "inner2": "value2"
            }})
        );
    }

    #[test]
    fn test_parse_values_from_file_throws_error_invalid_yaml() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let _ = temp_file.write(b"key1: value1\nkey2 value2").unwrap();
        assert!(parse_values(format!("fs://{}", temp_file.path().display())).is_err());
    }
}
