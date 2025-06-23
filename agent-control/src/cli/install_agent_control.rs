use crate::agent_control::config::{helmrelease_v2_type_meta, helmrepository_type_meta};
use crate::agent_control::defaults::AGENT_CONTROL_ID;
use crate::cli::errors::CliError;
use crate::cli::utils::*;
use crate::health::health_checker::HealthChecker;
use crate::health::k8s::health_checker::K8sHealthChecker;
use crate::health::with_start_time::StartTime;
use crate::k8s::annotations::Annotations;
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::k8s::labels::{AGENT_CONTROL_VERSION_SET_FROM, LOCAL_VAL, Labels, REMOTE_VAL};
use crate::sub_agent::identity::AgentIdentity;
use clap::Parser;
use kube::{
    Resource,
    api::{DynamicObject, ObjectMeta},
};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;
use tracing::{debug, info};

pub const RELEASE_NAME: &str = AC_DEPLOYMENT_CHART_NAME;
pub const REPOSITORY_NAME: &str = AGENT_CONTROL_ID;
const REPOSITORY_URL: &str = "https://helm-charts.newrelic.com";
const FIVE_MINUTES: &str = "300s";
const AC_DEPLOYMENT_CHART_NAME: &str = "agent-control-deployment";
const INSTALLATION_CHECK_DEFAULT_INITIAL_DELAY: &str = "10s";
const INSTALLATION_CHECK_DEFAULT_TIMEOUT: &str = "5m";
const INSTALLATION_CHECK_DEFAULT_RETRY_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Parser)]
pub struct AgentControlInstallData {
    /// Version of the agent control deployment chart
    #[arg(long)]
    pub chart_version: String,

    /// Secret values
    ///
    /// List of secret names and values keys to be used in the Helm release.
    /// Duplicate names are allowed.
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
    /// Multiple labels with the same name are **NOT** allowed. Only one label
    /// will be kept for each name.
    ///
    /// **Format**: label1=value1,label2=value2.
    ///
    /// [k8s labels]: https://kubernetes.io/docs/concepts/overview/working-with-objects/labels/#syntax-and-character-set
    #[arg(long)]
    pub extra_labels: Option<String>,

    /// Skip the installation check if set
    #[arg(long)]
    pub skip_installation_check: bool,

    /// Timeout for installation check
    #[arg(long, default_value = INSTALLATION_CHECK_DEFAULT_TIMEOUT, value_parser = parse_duration_arg)]
    pub installation_check_timeout: Duration,

    /// Initial delay for installation check
    #[arg(long, default_value = INSTALLATION_CHECK_DEFAULT_INITIAL_DELAY, value_parser = parse_duration_arg)]
    pub installation_check_initial_delay: Duration,

    /// Repository URl from where the chart will be downloaded
    #[arg(long, default_value = REPOSITORY_URL)]
    pub repository_url: String,
}

// helper needed because the arguments from the duration_str's parse function and the one expected by the clap
// `value_parser` argument have incompatible lifetimes.
fn parse_duration_arg(arg: &str) -> Result<Duration, String> {
    duration_str::parse(arg)
}

pub fn install_or_upgrade_agent_control(
    data: AgentControlInstallData,
    namespace: String,
) -> Result<(), CliError> {
    info!("Installing agent control");
    let k8s_client = try_new_k8s_client(namespace.clone())?;
    let maybe_helm_release = k8s_client
        .get_dynamic_object(&helmrelease_v2_type_meta(), RELEASE_NAME)
        .map_err(|err| {
            CliError::ApplyResource(format!(
                "could not get helmRelease with name {RELEASE_NAME}: {err}",
            ))
        })?;

    let dynamic_objects = build_dynamic_object_list(maybe_helm_release, data.clone());

    info!("Applying agent control resources");
    for object in dynamic_objects.iter() {
        apply_resource(&k8s_client, object)?;
    }
    info!("Agent control resources applied successfully");

    if !data.skip_installation_check {
        info!("Checking Agent control installation");
        check_installation(
            k8s_client,
            data.installation_check_timeout,
            data.installation_check_initial_delay,
            dynamic_objects,
        )?;
        info!("Agent control installed successfully");
    }

    Ok(())
}

fn apply_resource(k8s_client: &SyncK8sClient, object: &DynamicObject) -> Result<(), CliError> {
    let name = object.meta().name.clone().expect("Name should be present");
    let tm = object.types.clone().expect("Type should be present");

    info!("Applying {} with name \"{}\"", tm.kind, name);
    k8s_client
        .apply_dynamic_object(object)
        .map_err(|err| CliError::ApplyResource(err.to_string()))?;
    info!("{} with name {} applied successfully", tm.kind, name);

    Ok(())
}

fn is_version_managed_remotely(maybe_obj: Option<Arc<DynamicObject>>) -> bool {
    if let Some(obj) = maybe_obj {
        if let Some(labels) = obj.metadata.clone().labels {
            if labels
                .get_key_value(AGENT_CONTROL_VERSION_SET_FROM)
                .is_some_and(|(_, v)| v == REMOTE_VAL)
            {
                return true;
            }
        }
    }
    false
}

fn get_local_or_remote_version(
    maybe_existing_helm_release: Option<Arc<DynamicObject>>,
    local_version: String,
) -> (String, String) {
    if !is_version_managed_remotely(maybe_existing_helm_release.clone())
        || maybe_existing_helm_release.is_none()
    {
        debug!("Using local version: {}", local_version);
        return (local_version, LOCAL_VAL.to_string());
    }

    let existing_helm_release = maybe_existing_helm_release.unwrap();
    let remote_version = existing_helm_release
        .data
        .get("spec")
        .and_then(|spec| spec.get("chart"))
        .and_then(|spec| spec.get("spec"))
        .and_then(|chart| {
            chart
                .get("version")
                // Passing through the str is needed to avoid quotes
                .map(|v| v.as_str().unwrap_or_default().to_string())
        });

    match remote_version {
        Some(version) => {
            debug!("Using remote version: {version}",);
            (version, REMOTE_VAL.to_string())
        }
        None => {
            debug!("Remote version not found, using local: {local_version}");
            (local_version, LOCAL_VAL.to_string())
        }
    }
}

fn build_dynamic_object_list(
    maybe_existing_helm_release: Option<Arc<DynamicObject>>,
    value: AgentControlInstallData,
) -> Vec<DynamicObject> {
    let (version, source) =
        get_local_or_remote_version(maybe_existing_helm_release, value.chart_version.clone());

    let agent_identity = AgentIdentity::new_agent_control_identity();

    let mut labels = Labels::new(&agent_identity.id);
    let extra_labels = parse_key_value_pairs(value.extra_labels.as_deref().unwrap_or_default());
    labels.append_extra_labels(&extra_labels);
    let labels = labels.get();
    debug!("Parsed labels: {:?}", labels);

    // This is not strictly necessary, but it helps to ensure that the labels are consistent
    let mut helm_release_labels = labels.clone();

    helm_release_labels.insert(AGENT_CONTROL_VERSION_SET_FROM.to_string(), source);

    let annotations = Annotations::new_agent_type_id_annotation(&agent_identity.agent_type_id);
    let annotations = annotations.get();

    vec![
        helm_repository(
            value.repository_url.clone(),
            labels.clone(),
            annotations.clone(),
        ),
        helm_release(&value, helm_release_labels, annotations, version.as_str()),
    ]
}

fn helm_repository(
    repository_url: String,
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
                "url": repository_url,
                "interval": FIVE_MINUTES,
            }
        }),
    }
}

fn helm_release(
    value: &AgentControlInstallData,
    labels: BTreeMap<String, String>,
    annotations: BTreeMap<String, String>,
    version: &str,
) -> DynamicObject {
    let mut data = serde_json::json!({
        "spec": {
            "interval": FIVE_MINUTES,
            "timeout": FIVE_MINUTES,
            "chart": {
                "spec": {
                    "chart": AC_DEPLOYMENT_CHART_NAME,
                    "version": version,
                    "sourceRef": {
                        "kind": "HelmRepository",
                        "name": REPOSITORY_NAME,
                    },
                    "interval": FIVE_MINUTES,
                },
            }
        }
    });

    if let Some(secrets) = value.secrets.as_deref() {
        data["spec"]["valuesFrom"] = secrets_to_json(secrets);
    }

    DynamicObject {
        types: Some(helmrelease_v2_type_meta()),
        metadata: ObjectMeta {
            name: Some(RELEASE_NAME.to_string()),
            labels: Some(labels),
            annotations: Some(annotations),
            ..Default::default()
        },
        data,
    }
}

fn secrets_to_json(secrets: &str) -> serde_json::Value {
    let pairs = secrets.split(',');
    let key_values = pairs.filter_map(|pair| pair.split_once('='));
    let items = key_values
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

fn check_installation(
    k8s_client: SyncK8sClient,
    timeout: Duration,
    initial_delay: Duration,
    objects: Vec<DynamicObject>,
) -> Result<(), CliError> {
    let health_checker =
        K8sHealthChecker::try_new(Arc::new(k8s_client), Arc::new(objects), StartTime::now())
            .map_err(|err| {
                CliError::InstallationCheck(format!("could not build health-checker: {err}"))
            })?
            .ok_or(CliError::InstallationCheck(
                "no resources to check health were found".to_string(),
            ))?;

    let max_retries = timeout.as_secs() / INSTALLATION_CHECK_DEFAULT_RETRY_INTERVAL.as_secs();

    // An initial delay is needed because the api-server can take a while to actually apply the changes and we could
    // perform the health check to previous objects which could lead to false positives.
    info!(
        "Waiting for installation check initial delay: {}s",
        initial_delay.as_secs()
    );

    sleep(initial_delay);

    let retry_err = |err| {
        CliError::InstallationCheck(format!(
            "installation check failed after {} seconds timeout ({} attempts): {}",
            timeout.as_secs(),
            max_retries,
            err,
        ))
    };

    info!(
        "Performing installation check with {} attempts and {}s check interval",
        max_retries,
        INSTALLATION_CHECK_DEFAULT_RETRY_INTERVAL.as_secs()
    );

    let health = health_checker
        .check_health_with_retry(max_retries, INSTALLATION_CHECK_DEFAULT_RETRY_INTERVAL)
        .map_err(|err| retry_err(err.to_string()))?;

    if let Some(err) = health.last_error() {
        return Err(retry_err(err));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCAL_TEST_VERSION: &str = "1.0.0";

    fn agent_control_data() -> AgentControlInstallData {
        AgentControlInstallData {
            chart_version: LOCAL_TEST_VERSION.to_string(),
            secrets: None,
            extra_labels: None,
            skip_installation_check: false,
            installation_check_initial_delay: Duration::from_secs(10),
            installation_check_timeout: Duration::from_secs(300),
            repository_url: REPOSITORY_URL.to_string(),
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

    fn release_object(version: &str, source: &str) -> DynamicObject {
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
                    (
                        AGENT_CONTROL_VERSION_SET_FROM.to_string(),
                        source.to_string(),
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
                            "version": version,
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
    fn test_existing_object_no_label() {
        let dynamic_objects = build_dynamic_object_list(
            Some(Arc::new(DynamicObject {
                types: None,
                metadata: ObjectMeta::default(),
                data: serde_json::json!({
                    "spec": {
                        "chart": {
                            "spec":{
                                "version": "1.2.3",
                            }
                        }
                    }
                }),
            })),
            agent_control_data(),
        );
        assert_eq!(
            dynamic_objects,
            vec![
                repository_object(),
                release_object(LOCAL_TEST_VERSION, LOCAL_VAL)
            ]
        );
    }

    #[test]
    fn test_existing_object_remote_label() {
        let remote_version = "1.2.3";

        let dynamic_objects = build_dynamic_object_list(
            Some(Arc::new(DynamicObject {
                types: None,
                metadata: ObjectMeta {
                    labels: Some(BTreeMap::from_iter(vec![(
                        AGENT_CONTROL_VERSION_SET_FROM.to_string(),
                        REMOTE_VAL.to_string(),
                    )])),
                    ..Default::default()
                },
                data: serde_json::json!({
                    "spec": {
                        "chart": {
                            "spec":{
                                "version": remote_version,
                            }
                        }
                    }
                }),
            })),
            agent_control_data(),
        );
        assert_eq!(
            dynamic_objects,
            vec![
                repository_object(),
                release_object(remote_version, REMOTE_VAL)
            ]
        );
    }

    #[test]
    fn test_existing_object_local_label() {
        let dynamic_objects = build_dynamic_object_list(
            Some(Arc::new(DynamicObject {
                types: None,
                metadata: ObjectMeta {
                    labels: Some(BTreeMap::from_iter(vec![(
                        AGENT_CONTROL_VERSION_SET_FROM.to_string(),
                        LOCAL_VAL.to_string(),
                    )])),
                    ..Default::default()
                },
                data: serde_json::json!({
                    "spec": {
                        "chart": {
                            "spec":{
                                "version": "1.2.3",
                            }
                        }
                    }
                }),
            })),
            agent_control_data(),
        );
        assert_eq!(
            dynamic_objects,
            vec![
                repository_object(),
                release_object(LOCAL_TEST_VERSION, LOCAL_VAL)
            ]
        );
    }

    #[test]
    fn test_to_dynamic_objects_no_values() {
        let dynamic_objects = build_dynamic_object_list(None, agent_control_data());
        assert_eq!(
            dynamic_objects,
            vec![
                repository_object(),
                release_object(LOCAL_TEST_VERSION, LOCAL_VAL)
            ]
        );
    }

    #[test]
    fn test_to_dynamic_objects_with_secrets() {
        let mut agent_control_data = agent_control_data();
        agent_control_data.secrets =
            Some("secret1=default.yaml,secret2=values.yaml,secret3=fixed.yaml".to_string());
        let dynamic_objects = build_dynamic_object_list(None, agent_control_data);

        let mut expected_release_object = release_object(LOCAL_TEST_VERSION, LOCAL_VAL);
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
        agent_control_data.extra_labels = Some("label1=value1,label2=value2".to_string());
        let dynamic_objects = build_dynamic_object_list(None, agent_control_data);

        let agent_identity = AgentIdentity::new_agent_control_identity();
        let mut labels: BTreeMap<String, String> = vec![
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
        .collect();

        let annotations = Some(
            vec![(
                "newrelic.io/agent-type-id".to_string(),
                agent_identity.agent_type_id.to_string(),
            )]
            .into_iter()
            .collect(),
        );

        let mut expected_repository_object = repository_object();
        expected_repository_object.metadata.labels = Some(labels.clone());
        expected_repository_object.metadata.annotations = annotations.clone();

        labels.insert(
            AGENT_CONTROL_VERSION_SET_FROM.to_string(),
            LOCAL_VAL.to_string(),
        );
        let mut expected_release_object = release_object(LOCAL_TEST_VERSION, LOCAL_VAL);
        expected_release_object.metadata.labels = Some(labels);
        expected_release_object.metadata.annotations = annotations;

        assert_eq!(
            dynamic_objects,
            vec![expected_repository_object, expected_release_object]
        );
    }

    #[test]
    fn test_secrets_to_json_allow_duplicates() {
        assert_eq!(
            secrets_to_json("secret1=fixed.yaml,secret1=global.yaml"),
            serde_json::json!([
            {
                "kind": "Secret",
                "name": "secret1",
                "valuesKey": "fixed.yaml",
            },
            {
                "kind": "Secret",
                "name": "secret1",
                "valuesKey": "global.yaml",
            }])
        );
    }
}
