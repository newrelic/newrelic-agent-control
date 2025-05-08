use std::{collections::BTreeMap, str::FromStr};

use clap::Parser;
use kube::{
    api::{DynamicObject, ObjectMeta, TypeMeta},
    core::Duration,
};
use newrelic_agent_control::agent_control::config::helmrelease_v2_type_meta;
use tracing::{debug, info};

use crate::utils::parse_key_value_pairs;

const REPOSITORY_NAME: &str = "newrelic";
const REPOSITORY_URL: &str = "https://helm-charts.newrelic.com";

#[derive(Debug, Parser)]
pub struct AgentControlData {
    /// Release name
    #[arg(long)]
    pub release_name: String,

    /// Version of the agent control chart
    #[arg(long)]
    pub chart_version: String,

    /// Secret values
    ///
    /// List of secret names and values keys to be used in the Helm release.
    ///
    /// **Format**: secret_name_1=values_key_1,secret_name_2=values_key_2.
    #[arg(long)]
    pub secrets: Option<String>,

    /// Identifying metadata
    ///
    /// Labels are used to select and find collection of objects.
    /// They will be applied to every resource created for Agent Control.
    ///
    /// **Format**: label1=value1,label2=value2.
    #[arg(long)]
    pub labels: Option<String>,

    /// Non-identifying metadata
    ///
    /// They will be applied to every resource created for Agent Control.
    ///
    /// **Format**: annotation1=value1,annotation2=value2.
    #[arg(long)]
    pub annotations: Option<String>,
}

impl From<AgentControlData> for Vec<DynamicObject> {
    fn from(value: AgentControlData) -> Vec<DynamicObject> {
        info!("Creating Agent Control resources representations");

        let labels = parse_key_value_pairs(value.labels.as_deref().unwrap_or_default());
        debug!("Parsed labels: {:?}", labels);

        let annotations = parse_key_value_pairs(value.annotations.as_deref().unwrap_or_default());
        debug!("Parsed annotations: {:?}", annotations);

        let repository_object = helm_repository(labels.clone(), annotations.clone());

        let secrets = parse_key_value_pairs(value.secrets.as_deref().unwrap_or_default());
        let release_object = helm_release(&value, secrets, labels, annotations);

        info!("Agent Control resources representations created");

        vec![repository_object, release_object]
    }
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

fn helm_release(
    value: &AgentControlData,
    secrets: Option<BTreeMap<String, String>>,
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

    let secrets_data = secrets.iter().flatten();
    let secrets_json = secrets_data
        .map(|(name, values_key)| {
            serde_json::json!({
                "kind": "Secret",
                "name": name,
                "valuesKey": values_key,
                "optional": true,
            })
        })
        .collect::<Vec<serde_json::Value>>();
    if !secrets_json.is_empty() {
        data["spec"]["valuesFrom"] = serde_json::json!(secrets_json);
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
    use newrelic_agent_control::agent_control::config::helmrelease_v2_type_meta;

    use super::*;

    const RELEASE_NAME: &str = "agent-control-deployment-release";
    const VERSION: &str = "1.0.0";

    fn agent_control_data() -> AgentControlData {
        AgentControlData {
            release_name: RELEASE_NAME.to_string(),
            chart_version: VERSION.to_string(),
            secrets: None,
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
        let dynamic_objects = Vec::<DynamicObject>::from(agent_control_data());
        assert_eq!(dynamic_objects, vec![repository_object(), release_object()]);
    }

    #[test]
    fn test_to_dynamic_objects_with_secrets() {
        let mut agent_control_data = agent_control_data();
        agent_control_data.secrets =
            Some("secret1=default.yaml,secret2=values.yaml,secret3=fixed.yaml".to_string());
        let dynamic_objects = Vec::<DynamicObject>::from(agent_control_data);

        let mut expected_release_object = release_object();
        expected_release_object.data["spec"]["valuesFrom"] = serde_json::json!([
        {
            "kind": "Secret",
            "name": "secret1",
            "valuesKey": "default.yaml",
            "optional": true,
        },
        {
            "kind": "Secret",
            "name": "secret2",
            "valuesKey": "values.yaml",
            "optional": true,
        },
        {
            "kind": "Secret",
            "name": "secret3",
            "valuesKey": "fixed.yaml",
            "optional": true
        }]);
        assert_eq!(
            dynamic_objects,
            vec![repository_object(), expected_release_object]
        );
    }

    #[test]
    fn test_to_dynamic_objects_with_labels_and_annotations() {
        let mut agent_control_data = agent_control_data();
        agent_control_data.labels = Some("label1=value1,label2=value2".to_string());
        agent_control_data.annotations = Some("annotation1=value1,annotation2=value2".to_string());
        let dynamic_objects = Vec::<DynamicObject>::from(agent_control_data);

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

        let mut expected_release_object = release_object();
        expected_release_object.metadata.labels = labels;
        expected_release_object.metadata.annotations = annotations;

        assert_eq!(
            dynamic_objects,
            vec![expected_repository_object, expected_release_object]
        );
    }
}
