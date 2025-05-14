use crate::agent_control::config::{helmrelease_v2_type_meta, helmrepository_type_meta};
use crate::cli::errors::CliError;
use crate::cli::utils::parse_key_value_pairs;
use crate::k8s::annotations::Annotations;
use crate::k8s::client::SyncK8sClient;
use crate::k8s::labels::Labels;
use crate::sub_agent::identity::AgentIdentity;
use clap::Parser;
use kube::{
    Resource,
    api::{DynamicObject, ObjectMeta},
    core::Duration,
};
use std::sync::Arc;
use std::{collections::BTreeMap, str::FromStr};
use tracing::{debug, info};

const REPOSITORY_NAME: &str = "newrelic";
const REPOSITORY_URL: &str = "https://helm-charts.newrelic.com";
const FIVE_MINUTES: &str = "5m";
const AC_DEPLOYMENT_CHART_NAME: &str = "agent-control-deployment";

#[derive(Debug, Parser)]
pub struct AgentControlInstallData {
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
    /// Check out [k8s labels] for more information on the restrictions and
    /// rules for labels names and values.
    ///
    /// **Format**: label1=value1,label2=value2.
    ///
    /// [k8s labels]: https://kubernetes.io/docs/concepts/overview/working-with-objects/labels/#syntax-and-character-set
    #[arg(long)]
    pub extra_labels: Option<String>,
}

pub fn install_agent_control(
    data: AgentControlInstallData,
    namespace: String,
) -> Result<(), CliError> {
    info!("Installing agent control");

    let dynamic_objects = Vec::<DynamicObject>::from(data);

    let k8s_client = k8s_client(namespace.clone())?;

    // TODO: Take care of upgrade.
    // For example, what happens if the user applies a remote configuration with a lower version
    // that includes a breaking change?
    info!("Applying agent control resources");
    for object in dynamic_objects {
        apply_resource(&k8s_client, &object)?;
    }
    info!("Agent control resources applied successfully");

    info!("Agent control installed successfully");

    Ok(())
}

fn apply_resource(k8s_client: &SyncK8sClient, object: &DynamicObject) -> Result<(), CliError> {
    let name = object.meta().name.clone().expect("Name should be present");
    let kind = object
        .types
        .clone()
        .map(|t| t.kind)
        .unwrap_or_else(|| "Unknown kind".to_string());

    info!("Applying {} with name \"{}\"", kind, name);
    k8s_client
        .apply_dynamic_object(object)
        .map_err(|err| CliError::ApplyResource(err.to_string()))?;
    info!("{} with name {} applied successfully", kind, name);

    Ok(())
}

fn k8s_client(namespace: String) -> Result<SyncK8sClient, CliError> {
    debug!("Starting the runtime");
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Tokio should be able to create a runtime"),
    );

    debug!("Starting the k8s client");
    SyncK8sClient::try_new(runtime, namespace).map_err(|err| CliError::K8sClient(err.to_string()))
}

impl From<AgentControlInstallData> for Vec<DynamicObject> {
    fn from(value: AgentControlInstallData) -> Vec<DynamicObject> {
        let agent_identity = AgentIdentity::new_agent_control_identity();

        let mut labels = Labels::new(&agent_identity.id);
        let extra_labels = parse_key_value_pairs(value.extra_labels.as_deref().unwrap_or_default());
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
    value: &AgentControlInstallData,
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
    use super::*;

    const RELEASE_NAME: &str = "agent-control-deployment-release";
    const VERSION: &str = "1.0.0";

    fn agent_control_data() -> AgentControlInstallData {
        AgentControlInstallData {
            release_name: RELEASE_NAME.to_string(),
            chart_version: VERSION.to_string(),
            secrets: None,
            extra_labels: None,
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
        assert_eq!(dynamic_objects, vec![
            repository_object(),
            expected_release_object
        ]);
    }

    #[test]
    fn test_to_dynamic_objects_with_labels_and_annotations() {
        let mut agent_control_data = agent_control_data();
        agent_control_data.extra_labels = Some("label1=value1,label2=value2".to_string());
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

        assert_eq!(dynamic_objects, vec![
            expected_repository_object,
            expected_release_object
        ]);
    }
}
