use std::{collections::BTreeMap, str::FromStr};

use clap::Parser;
use kube::{
    api::{DynamicObject, ObjectMeta},
    core::Duration,
};
use newrelic_agent_control::{
    agent_control::config::{helmrelease_v2_type_meta, helmrepository_type_meta},
    k8s::{annotations::Annotations, labels::Labels},
    sub_agent::identity::AgentIdentity,
};
use tracing::debug;

use crate::utils::parse_key_value_pairs;

const REPOSITORY_NAME: &str = "newrelic";
const REPOSITORY_URL: &str = "https://helm-charts.newrelic.com";
const FIVE_MINUTES: &str = "5m";
const AC_DEPLOYMENT_CHART_NAME: &str = "agent-control-deployment";

#[derive(Debug, Parser)]
pub struct AgentControlData {
    /// Release name
    #[arg(long)]
    pub release_name: String,

    /// Version of the agent control deployment chart
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
}

impl From<AgentControlData> for Vec<DynamicObject> {
    fn from(value: AgentControlData) -> Vec<DynamicObject> {
        let agent_identity = AgentIdentity::new_agent_control_identity();

        let mut labels = Labels::new(&agent_identity.id);
        let extra_labels = parse_key_value_pairs(value.labels.as_deref().unwrap_or_default());
        labels.append_extra_labels(&extra_labels);
        let labels = labels.get();
        debug!("Parsed labels: {:?}", labels);

        let annotations = Annotations::new_agent_type_id_annotation(&agent_identity.agent_type_id);
        let annotations = annotations.get();

        let repository_object = helm_repository(labels.clone(), annotations.clone());

        let secrets = parse_key_value_pairs(value.secrets.as_deref().unwrap_or_default());
        let release_object = helm_release(&value, secrets, labels, annotations);

        vec![repository_object, release_object]
    }
}

fn helm_repository(
    labels: BTreeMap<String, String>,
    annotations: BTreeMap<String, String>,
) -> DynamicObject {
    DynamicObject {
        types: Some(helmrepository_type_meta()),
        metadata: ObjectMeta {
            name: Some(REPOSITORY_NAME.to_string()),
            labels: Some(labels),
            annotations: Some(annotations),
            ..Default::default()
        },
        data: serde_json::json!({
            "spec": {
                "url": REPOSITORY_URL,
                "interval": Duration::from_str(FIVE_MINUTES).expect("Hardcoded value should be correct"),
            }
        }),
    }
}

fn helm_release(
    value: &AgentControlData,
    secrets: BTreeMap<String, String>,
    labels: BTreeMap<String, String>,
    annotations: BTreeMap<String, String>,
) -> DynamicObject {
    let interval = Duration::from_str(FIVE_MINUTES).expect("Hardcoded value should be correct");
    let timeout = Duration::from_str(FIVE_MINUTES).expect("Hardcoded value should be correct");
    let mut data = serde_json::json!({
        "spec": {
            "interval": interval,
            "timeout": timeout,
            "chart": {
                "spec": {
                    "chart": AC_DEPLOYMENT_CHART_NAME,
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

    if !secrets.is_empty() {
        data["spec"]["valuesFrom"] = secrets_to_json(secrets);
    }

    DynamicObject {
        types: Some(helmrelease_v2_type_meta()),
        metadata: ObjectMeta {
            name: Some(value.release_name.clone()),
            labels: Some(labels),
            annotations: Some(annotations),
            ..Default::default()
        },
        data,
    }
}

fn secrets_to_json(secrets: BTreeMap<String, String>) -> serde_json::Value {
    let items = secrets
        .iter()
        .map(|(name, values_key)| {
            serde_json::json!({
                "kind": "Secret",
                "name": name,
                "valuesKey": values_key,
            })
        })
        .collect::<Vec<serde_json::Value>>();

    serde_json::json!(items)
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
        }
    }

    fn repository_object() -> DynamicObject {
        let agent_identity = AgentIdentity::new_agent_control_identity();

        DynamicObject {
            types: Some(helmrepository_type_meta()),
            metadata: ObjectMeta {
                name: Some(REPOSITORY_NAME.to_string()),
                labels: Some(BTreeMap::from_iter(vec![
                    (
                        "app.kubernetes.io/managed-by".to_string(),
                        "newrelic-agent-control".to_string(),
                    ),
                    (
                        "newrelic.io/agent-id".to_string(),
                        agent_identity.id.to_string(),
                    ),
                ])),
                annotations: Some(BTreeMap::from_iter(vec![(
                    "newrelic.io/agent-type-id".to_string(),
                    agent_identity.agent_type_id.to_string(),
                )])),
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
        let agent_identity = AgentIdentity::new_agent_control_identity();

        DynamicObject {
            types: Some(helmrelease_v2_type_meta()),
            metadata: ObjectMeta {
                name: Some(RELEASE_NAME.to_string()),
                labels: Some(BTreeMap::from_iter(vec![
                    (
                        "app.kubernetes.io/managed-by".to_string(),
                        "newrelic-agent-control".to_string(),
                    ),
                    (
                        "newrelic.io/agent-id".to_string(),
                        agent_identity.id.to_string(),
                    ),
                ])),
                annotations: Some(BTreeMap::from_iter(vec![(
                    "newrelic.io/agent-type-id".to_string(),
                    agent_identity.agent_type_id.to_string(),
                )])),
                ..Default::default()
            },
            data: serde_json::json!({
                "spec": {
                    "interval": "300s",
                    "timeout": "300s",
                    "chart": {
                        "spec": {
                            "chart": AC_DEPLOYMENT_CHART_NAME,
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
        },
        {
            "kind": "Secret",
            "name": "secret2",
            "valuesKey": "values.yaml",
        },
        {
            "kind": "Secret",
            "name": "secret3",
            "valuesKey": "fixed.yaml",
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
        let dynamic_objects = Vec::<DynamicObject>::from(agent_control_data);

        let agent_identity = AgentIdentity::new_agent_control_identity();
        let labels = Some(
            vec![
                (
                    "app.kubernetes.io/managed-by".to_string(),
                    "newrelic-agent-control".to_string(),
                ),
                (
                    "newrelic.io/agent-id".to_string(),
                    agent_identity.id.to_string(),
                ),
                ("label1".to_string(), "value1".to_string()),
                ("label2".to_string(), "value2".to_string()),
            ]
            .into_iter()
            .collect(),
        );
        let annotations = Some(
            vec![(
                "newrelic.io/agent-type-id".to_string(),
                agent_identity.agent_type_id.to_string(),
            )]
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
