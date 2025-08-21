pub mod agent_control;
pub mod flux;

use std::{collections::BTreeMap, sync::Arc, thread, time::Duration};

use clap::{Parser, arg};
use kube::api::{DynamicObject, ObjectMeta};
use tracing::{debug, info};

#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::{
    agent_control::config::{helmrelease_v2_type_meta, helmrepository_type_meta},
    cli::{errors::CliError, utils::try_new_k8s_client},
    health::{
        health_checker::HealthChecker, k8s::health_checker::K8sHealthChecker,
        with_start_time::StartTime,
    },
    k8s::{
        labels::{AGENT_CONTROL_VERSION_SET_FROM, LOCAL_VAL, REMOTE_VAL},
        utils::{get_name, get_type_meta},
    },
};

const REPOSITORY_URL: &str = "https://helm-charts.newrelic.com";
const INSTALLATION_CHECK_DEFAULT_INITIAL_DELAY: &str = "10s";
const INSTALLATION_CHECK_DEFAULT_TIMEOUT: &str = "5m";
const INSTALLATION_CHECK_DEFAULT_RETRY_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Parser)]
pub struct InstallData {
    /// Name of the Helm chart to be installed
    #[arg(long)]
    pub chart_name: String,

    /// Version of the agent control deployment chart
    #[arg(long)]
    pub chart_version: String,

    /// Name of the Helm release
    #[arg(long)]
    pub release_name: String,

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

    /// Optional Flux HelmRepository secret reference (secretRef)
    /// More details: https://fluxcd.io/flux/components/source/helmrepositories/#secret-reference
    #[arg(long)]
    pub repository_secret_reference_name: Option<String>,

    /// Optional Flux HelmRepository certificate secret reference (certSecretRef)
    /// More details: https://fluxcd.io/flux/components/source/helmrepositories/#cert-secret-reference
    #[arg(long)]
    pub repository_certificate_secret_reference_name: Option<String>,
}

/// Structures able to emit a list of dynamic objects can be members of this trait.
pub trait DynamicObjectListBuilder {
    /// Build a list of dynamic objects to be applied
    fn build_dynamic_object_list(
        &self,
        namespace: &str,
        release_name: &str,
        maybe_existing_helm_release: Option<&DynamicObject>,
        data: &InstallData,
    ) -> Vec<DynamicObject>;
}

// helper needed because the arguments from the duration_str's parse function and the one expected by the clap
// `value_parser` argument have incompatible lifetimes.
fn parse_duration_arg(arg: &str) -> Result<Duration, String> {
    duration_str::parse(arg)
}

pub fn apply_resources(
    dyn_object_list_builder: impl DynamicObjectListBuilder,
    namespace: &str,
    install_data: &InstallData,
) -> Result<(), CliError> {
    let release_name = &install_data.release_name;
    info!("Installing release {release_name}");
    let k8s_client = try_new_k8s_client()?;
    let maybe_helm_release = k8s_client
        .get_dynamic_object(&helmrelease_v2_type_meta(), release_name, namespace)
        .map_err(|err| {
            CliError::ApplyResource(format!(
                "could not get helmRelease with name {release_name}: {err}",
            ))
        })?;

    let skip_installation_check = install_data.skip_installation_check;
    let installation_check_timeout = install_data.installation_check_timeout;
    let installation_check_initial_delay = install_data.installation_check_initial_delay;

    let maybe_existing_helm_release = maybe_helm_release.as_ref().map(|o| o.as_ref());
    let dynamic_objects = dyn_object_list_builder.build_dynamic_object_list(
        namespace,
        release_name,
        maybe_existing_helm_release,
        install_data,
    );

    info!("Applying release {release_name} resources");
    dynamic_objects
        .iter()
        .try_for_each(|obj| apply_resource(&k8s_client, obj))?;
    info!("Release {release_name} resources applied successfully");

    if !skip_installation_check {
        info!("Checking release {release_name} installation");
        check_installation(
            k8s_client,
            installation_check_timeout,
            installation_check_initial_delay,
            dynamic_objects,
        )?;
        info!("Release {release_name} installed successfully");
    }

    Ok(())
}

fn apply_resource(k8s_client: &SyncK8sClient, object: &DynamicObject) -> Result<(), CliError> {
    let name = get_name(object).expect("name is expected to be present");
    let tm = get_type_meta(object).expect("type is expected to be present");

    info!("Applying {} with name \"{}\"", tm.kind, name);
    k8s_client
        .apply_dynamic_object(object)
        .map_err(|err| CliError::ApplyResource(err.to_string()))?;
    info!("{} with name {} applied successfully", tm.kind, name);

    Ok(())
}

fn is_version_managed_remotely(obj: &DynamicObject) -> bool {
    obj.metadata.labels.as_ref().is_some_and(|labels| {
        labels
            .get(AGENT_CONTROL_VERSION_SET_FROM)
            .is_some_and(|v| v == REMOTE_VAL)
    })
}

fn get_local_or_remote_version(
    maybe_existing_helm_release: Option<&DynamicObject>,
    chart_version: String,
) -> (String, String) {
    maybe_existing_helm_release
        .filter(|obj| is_version_managed_remotely(obj))
        .and_then(get_remote_version)
        .map(|remote_version| {
            debug!("Using remote version: {remote_version}");
            (remote_version, REMOTE_VAL.to_string())
        })
        .unwrap_or_else(|| {
            debug!("Using local version: {}", &chart_version);
            (chart_version, LOCAL_VAL.to_string())
        })
}

fn get_remote_version(helm_release: &DynamicObject) -> Option<String> {
    let remote_version = helm_release
        .data
        .get("spec")
        .and_then(|spec| spec.get("chart"))
        .and_then(|spec| spec.get("spec"))
        .and_then(|chart| chart.get("version"))
        // Passing through the str is needed to avoid quotes
        .and_then(|v| v.as_str())
        .map(|s| s.to_owned());

    if remote_version.is_none() {
        debug!("Remote version not found in HelmRelease");
    }

    remote_version
}

fn obj_meta_data(
    name: &str,
    namespace: &str,
    labels: BTreeMap<String, String>,
    annotations: BTreeMap<String, String>,
) -> ObjectMeta {
    ObjectMeta {
        name: Some(name.to_string()),
        namespace: Some(namespace.to_string()),
        labels: Some(labels),
        annotations: Some(annotations),
        ..Default::default()
    }
}

fn helm_repository(
    repository_url: &str,
    maybe_secret_ref: Option<String>,
    maybe_cert_secret_ref: Option<String>,
    obj_meta_data: ObjectMeta,
) -> DynamicObject {
    let secret_ref = maybe_secret_ref.map(|name| serde_json::json!({"name": name}));
    let cert_secret_ref = maybe_cert_secret_ref.map(|name| serde_json::json!({"name": name}));
    DynamicObject {
        types: Some(helmrepository_type_meta()),
        metadata: obj_meta_data,
        // See com.newrelic.infrastructure Agent type for description of fields.
        data: serde_json::json!({
            "spec": {
                "url": repository_url,
                "interval": "30m",
                "provider": "generic",
                "secretRef": secret_ref,
                "certSecretRef": cert_secret_ref,
            }
        }),
    }
}

fn helm_release(
    values_secrets: &Option<String>,
    repository_name: &str,
    version: &str,
    chart_name: &str,
    obj_meta_data: ObjectMeta,
) -> DynamicObject {
    // See com.newrelic.infrastructure Agent type for description of fields.
    let mut data = serde_json::json!({
        "spec": {
            "interval": "30s",
            "releaseName": obj_meta_data.name.as_ref(),
            "chart": {
                "spec": {
                    "chart": chart_name,
                    "version": version,
                    "reconcileStrategy": "ChartVersion",
                    "sourceRef": {
                        "kind": "HelmRepository",
                        "name": repository_name,
                    },
                    "interval": "3m",
                },
            },
            "install": {
                "disableWait": true,
                "disableWaitForJobs": true,
                "disableTakeOwnership": true,
            },
            "upgrade": {
                "disableWait": true,
                "disableWaitForJobs": true,
                "disableTakeOwnership": true,
                "cleanupOnFail": true,
            },
            "rollback": {
                "disableWait": true,
                "disableWaitForJobs": true
            },
        }
    });

    if let Some(secrets) = values_secrets.as_ref() {
        data["spec"]["valuesFrom"] = secrets_to_json(secrets);
    }

    DynamicObject {
        types: Some(helmrelease_v2_type_meta()),
        metadata: obj_meta_data,
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

    thread::sleep(initial_delay);

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
    use crate::{
        agent_control::config::helmrepository_type_meta,
        cli::install::agent_control::InstallAgentControl,
        k8s::labels::{AGENT_CONTROL_VERSION_SET_FROM, LOCAL_VAL, REMOTE_VAL},
        sub_agent::identity::AgentIdentity,
    };

    use super::*;

    const LOCAL_TEST_VERSION: &str = "1.0.0";
    const TEST_NAMESPACE: &str = "test-namespace";
    const RELEASE_NAME: &str = "test-release-name";

    fn ac_install_data() -> InstallData {
        InstallData {
            chart_name: RELEASE_NAME.to_string(),
            chart_version: LOCAL_TEST_VERSION.to_string(),
            release_name: RELEASE_NAME.to_string(),
            secrets: None,
            extra_labels: None,
            skip_installation_check: false,
            installation_check_initial_delay: Duration::from_secs(10),
            installation_check_timeout: Duration::from_secs(300),
            repository_url: REPOSITORY_URL.to_string(),
            repository_secret_reference_name: None,
            repository_certificate_secret_reference_name: None,
        }
    }

    fn repository_object() -> DynamicObject {
        let agent_identity = AgentIdentity::new_agent_control_identity();

        DynamicObject {
            types: Some(helmrepository_type_meta()),
            metadata: ObjectMeta {
                name: Some(agent_control::REPOSITORY_NAME.to_string()),
                namespace: Some(TEST_NAMESPACE.to_string()),
                labels: Some(BTreeMap::from_iter([
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
                ..ObjectMeta::default()
            },
            data: serde_json::json!({
                "spec": {
                    "url": REPOSITORY_URL,
                    "interval": "30m",
                    "provider": "generic",
                    "secretRef": serde_json::Value::Null,
                    "certSecretRef": serde_json::Value::Null,
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
                namespace: Some(TEST_NAMESPACE.to_string()),
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
                    "interval": "30s",
                    "releaseName": RELEASE_NAME,
                    "chart": {
                        "spec": {
                            "chart": RELEASE_NAME,
                            "version": version,
                            "reconcileStrategy": "ChartVersion",
                            "sourceRef": {
                                "kind": "HelmRepository",
                                "name": agent_control::REPOSITORY_NAME,
                            },
                            "interval": "3m",
                        },
                    },
                    "install": {
                        "disableWait": true,
                        "disableWaitForJobs": true,
                        "disableTakeOwnership": true,
                    },
                    "upgrade": {
                        "disableWait": true,
                        "disableWaitForJobs": true,
                        "disableTakeOwnership": true,
                        "cleanupOnFail": true,
                    },
                    "rollback": {
                        "disableWait": true,
                        "disableWaitForJobs": true
                    },
                }
            }),
        }
    }

    #[test]
    fn test_existing_object_no_label() {
        let dynamic_objects = InstallAgentControl.build_dynamic_object_list(
            TEST_NAMESPACE,
            RELEASE_NAME,
            Some(&DynamicObject {
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
            }),
            &ac_install_data(),
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

        let dynamic_objects = InstallAgentControl.build_dynamic_object_list(
            TEST_NAMESPACE,
            RELEASE_NAME,
            Some(&DynamicObject {
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
            }),
            &ac_install_data(),
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
        let dynamic_objects = InstallAgentControl.build_dynamic_object_list(
            TEST_NAMESPACE,
            RELEASE_NAME,
            Some(&DynamicObject {
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
            }),
            &ac_install_data(),
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
        let dynamic_objects = InstallAgentControl.build_dynamic_object_list(
            TEST_NAMESPACE,
            RELEASE_NAME,
            None,
            &ac_install_data(),
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
    fn test_to_dynamic_objects_with_secrets() {
        let agent_control_data = InstallData {
            secrets: Some(
                "secret1=default.yaml,secret2=values.yaml,secret3=fixed.yaml".to_string(),
            ),
            ..ac_install_data()
        };
        let dynamic_objects = InstallAgentControl.build_dynamic_object_list(
            TEST_NAMESPACE,
            RELEASE_NAME,
            None,
            &agent_control_data,
        );

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
        let agent_control_data = InstallData {
            extra_labels: Some("label1=value1,label2=value2".to_string()),
            ..ac_install_data()
        };
        let dynamic_objects = InstallAgentControl.build_dynamic_object_list(
            TEST_NAMESPACE,
            RELEASE_NAME,
            None,
            &agent_control_data,
        );

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

    #[test]
    fn test_secret_ref() {
        let dynamic_objects = InstallAgentControl.build_dynamic_object_list(
            TEST_NAMESPACE,
            RELEASE_NAME,
            None,
            &InstallData {
                repository_secret_reference_name: Some("secRef".to_string()),
                repository_certificate_secret_reference_name: Some("certSecRef".to_string()),
                ..ac_install_data()
            },
        );

        assert!(
            dynamic_objects.iter().any(|obj| {
                obj.data["spec"]["secretRef"]["name"].eq(&serde_json::json!("secRef"))
            })
        );
        assert!(dynamic_objects.iter().any(|obj| {
            obj.data["spec"]["certSecretRef"]["name"].eq(&serde_json::json!("certSecRef"))
        }));
    }

    #[test]
    fn test_chart_name() {
        let chart_name = "my-chart";
        let agent_control_data = InstallData {
            chart_name: chart_name.to_string(),
            ..ac_install_data()
        };
        let dynamic_objects = InstallAgentControl.build_dynamic_object_list(
            TEST_NAMESPACE,
            RELEASE_NAME,
            None,
            &agent_control_data,
        );

        assert!(dynamic_objects.iter().any(|obj| {
            obj.data["spec"]["chart"]["spec"]["chart"].eq(&serde_json::json!(chart_name))
        }));
    }
}
