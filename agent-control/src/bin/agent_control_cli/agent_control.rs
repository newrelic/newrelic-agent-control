use std::{collections::BTreeMap, str::FromStr};

use clap::Parser;
use kube::{
    api::{DynamicObject, ObjectMeta, TypeMeta},
    core::Duration,
};
use newrelic_agent_control::agent_control::config::helmrelease_v2_type_meta;
use tracing::{debug, info};

use crate::{errors::ParseError, utils::parse_key_value_pairs};

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

        let repository_object = helm_repository(labels.clone(), annotations.clone());

        let values = value.values.clone().map(parse_values).transpose()?;
        let secret_object =
            values.map(|values| secret(values, labels.clone(), annotations.clone()));

        let release_object = helm_release(
            &value,
            secret_object.clone().and(Some(SECRET_NAME.to_string())),
            labels,
            annotations,
        );

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

fn helmrepository_type_meta() -> TypeMeta {
    TypeMeta {
        api_version: "source.toolkit.fluxcd.io/v1".to_string(),
        kind: "HelmRepository".to_string(),
    }
}

fn helm_repository(
    labels: Option<BTreeMap<String, String>>,
    annotations: Option<BTreeMap<String, String>>,
) -> DynamicObject {
    info!(
        "Creating Helm repository representation with name \"{}\"",
        REPOSITORY_NAME
    );
    let dynamic_object = DynamicObject {
        types: Some(helmrepository_type_meta()),
        metadata: ObjectMeta {
            name: Some(REPOSITORY_NAME.to_string()),
            labels,
            annotations,
            ..Default::default()
        },
        data: serde_json::json!({
            "spec": {
                "url": REPOSITORY_URL,
                "interval": Duration::from_str("5m").expect("Hardcoded value should be correct"),
            }
        }),
    };
    info!(
        "Helm repository representation with name \"{}\" created",
        REPOSITORY_NAME
    );

    dynamic_object
}

fn secret_type_meta() -> TypeMeta {
    TypeMeta {
        api_version: "v1".to_string(),
        kind: "Secret".to_string(),
    }
}

fn secret(
    values: serde_json::Value,
    labels: Option<BTreeMap<String, String>>,
    annotations: Option<BTreeMap<String, String>>,
) -> DynamicObject {
    info!(
        "Creating Secret representation with name \"{}\"",
        SECRET_NAME
    );

    let dynamic_object = DynamicObject {
        types: Some(secret_type_meta()),
        metadata: ObjectMeta {
            name: Some(SECRET_NAME.to_string()),
            labels,
            annotations,
            ..Default::default()
        },
        data: serde_json::json!({
            "type": "Opaque",
            "stringData": {
                "values.yaml": values.to_string()
            }
        }),
    };

    info!(
        "Secret representation with name \"{}\" created",
        SECRET_NAME
    );

    dynamic_object
}

fn helm_release(
    value: &AgentControlData,
    secret_name: Option<String>,
    labels: Option<BTreeMap<String, String>>,
    annotations: Option<BTreeMap<String, String>>,
) -> DynamicObject {
    info!(
        "Creating Helm release representation with name \"{}\"",
        value.release_name
    );

    let interval = Duration::from_str("5m").expect("Hardcoded value should be correct");
    let timeout = Duration::from_str("5m").expect("Hardcoded value should be correct");

    let mut data = serde_json::json!({
        "spec": {
            "interval": interval,
            "timeout": timeout,
            "chart": {
                "spec": {
                    "chart": "agent-control-deployment",
                    "version": value.chart_version,
                    "sourceRef": {
                        "kind": "HelmRepository",
                        "name": REPOSITORY_NAME,
                    },
                    "interval": interval,
                },
            }
        }
    });

    if let Some(secret_name) = secret_name {
        data["spec"]["valuesFrom"] = serde_json::json!([{
            "kind": "Secret",
            "name": secret_name,
            "valuesKey": "values.yaml",
        }]);
    }

    let dynamic_object = DynamicObject {
        types: Some(helmrelease_v2_type_meta()),
        metadata: ObjectMeta {
            name: Some(value.release_name.clone()),
            labels,
            annotations,
            ..Default::default()
        },
        data,
    };
    info!(
        "Helm release representation with name \"{}\" created",
        value.release_name
    );

    dynamic_object
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use newrelic_agent_control::agent_control::config::helmrelease_v2_type_meta;
    use tempfile::NamedTempFile;

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
