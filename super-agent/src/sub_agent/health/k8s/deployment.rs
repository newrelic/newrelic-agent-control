#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::k8s::error::K8sError;
use crate::k8s::utils::IntOrPercentage;
use crate::sub_agent::health::health_checker::{
    Health, HealthChecker, HealthCheckerError, Healthy,
};
use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec, ReplicaSet};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::core::ObjectMeta;
use std::sync::Arc;

use super::items::{check_health_for_items, flux_release_filter};

const ROLLING_UPDATE: &str = "RollingUpdate";
const DEPLOYMENT_KIND: &str = "Deployment";

pub struct K8sHealthDeployment {
    k8s_client: Arc<SyncK8sClient>,
    release_name: String,
}

impl HealthChecker for K8sHealthDeployment {
    fn check_health(&self) -> Result<Health, HealthCheckerError> {
        let deployments = self.k8s_client.list_deployment();

        let target_deployments = deployments
            .into_iter()
            .filter(flux_release_filter(self.release_name.clone()));

        check_health_for_items(target_deployments, |deployment: Arc<Deployment>| {
            let name = Self::get_metadata_name(&deployment.metadata)?;
            self.latest_replica_set_for_deployment(name.as_str())
                .map(|replica_set| Self::check_deployment_health(deployment, replica_set))
                .unwrap_or_else(|| {
                    Ok(Health::unhealthy_with_last_error(format!(
                        "ReplicaSet not found for Deployment {name}"
                    )))
                })
        })
    }
}

impl K8sHealthDeployment {
    pub fn new(k8s_client: Arc<SyncK8sClient>, release_name: String) -> Self {
        Self {
            k8s_client,
            release_name,
        }
    }

    fn missing_field_error(name: &str, field: &str) -> HealthCheckerError {
        HealthCheckerError::MissingK8sObjectField {
            kind: DEPLOYMENT_KIND.to_string(),
            name: name.to_string(),
            field: field.to_string(),
        }
    }

    /// Checks the health of a specific deployment and its associated ReplicaSet.
    pub fn check_deployment_health(
        deployment: Arc<Deployment>,
        rs: Arc<ReplicaSet>,
    ) -> Result<Health, HealthCheckerError> {
        let name = Self::get_metadata_name(&deployment.metadata)?;

        let status = deployment
            .status
            .as_ref()
            .ok_or_else(|| Self::missing_field_error(&name, "status"))?;

        let spec = deployment
            .spec
            .as_ref()
            .ok_or_else(|| Self::missing_field_error(&name, "spec"))?;

        // If the deployment is paused, consider it unhealthy
        if let Some(true) = spec.paused {
            return Ok(Health::unhealthy_with_last_error(format!(
                "Deployment '{}' is paused",
                name,
            )));
        }

        let replicas = status
            .replicas
            .ok_or_else(|| Self::missing_field_error(&name, "status.replicas"))?;

        let max_unavailable = Self::max_unavailable(&name, spec)?;

        let expected_ready = replicas.checked_sub(max_unavailable).ok_or_else(|| {
            HealthCheckerError::Generic(format!(
                "Invalid calculation for expected ready replicas for Deployment '{}'",
                name
            ))
        })?;

        let rs_status = rs
            .status
            .as_ref()
            .ok_or_else(|| Self::missing_field_error(&name, "replica set status"))?;

        let ready_replicas = rs_status
            .ready_replicas
            .ok_or_else(|| Self::missing_field_error(&name, "ready replicas"))?;

        if ready_replicas < expected_ready {
            return Ok(Health::unhealthy_with_last_error(format!(
                "Deployment '{}' is not ready. {} out of {} expected pods are ready",
                name, ready_replicas, expected_ready
            )));
        }

        Ok(Healthy::default().into())
    }

    fn get_metadata_name(metadata: &ObjectMeta) -> Result<String, HealthCheckerError> {
        metadata.name.clone().ok_or_else(|| {
            HealthCheckerError::K8sError(K8sError::MissingName("Deployment".to_string()))
        })
    }

    /// Calculates the maximum number of unavailable pods during a rolling update.
    fn max_unavailable(
        metadata_name: &str,
        spec: &DeploymentSpec,
    ) -> Result<i32, HealthCheckerError> {
        let replicas = spec
            .replicas
            .ok_or_else(|| Self::missing_field_error(metadata_name, "spec.replicas"))?;

        if !Self::is_rolling_update(metadata_name, spec)? || replicas == 0 {
            return Ok(0);
        }

        let rolling_update_strategy = spec
            .strategy
            .as_ref()
            .and_then(|strategy| strategy.rolling_update.as_ref());
        let max_surge = rolling_update_strategy.and_then(|ru| ru.max_surge.as_ref());
        let max_unavailable = rolling_update_strategy.and_then(|ru| ru.max_unavailable.as_ref());

        let max_unavailable =
            Self::resolve_fenceposts(max_surge, max_unavailable, metadata_name, replicas)?;

        Ok(max_unavailable.min(replicas))
    }

    /// Checks if the deployment strategy is a rolling update.
    fn is_rolling_update(
        metadata_name: &str,
        spec: &DeploymentSpec,
    ) -> Result<bool, HealthCheckerError> {
        let strategy = spec
            .strategy
            .as_ref()
            .ok_or_else(|| Self::missing_field_error(metadata_name, "spec.strategy"))?;

        let type_ = strategy
            .type_
            .as_deref()
            .ok_or_else(|| Self::missing_field_error(metadata_name, "spec.strategy.type"))?;

        Ok(type_ == ROLLING_UPDATE)
    }

    /// Return the maximum number of unavailable pods during a rolling update.
    ///
    /// Ensures the calculated value is within reasonable bounds and defaults to 1 if both
    /// max_surge and max_unavailable resolve to zero.
    fn resolve_fenceposts(
        max_surge: Option<&IntOrString>,
        max_unavailable: Option<&IntOrString>,
        name: &str,
        desired: i32,
    ) -> Result<i32, HealthCheckerError> {
        let surge = Self::int_or_string_to_value(max_surge, name, "max_surge", desired, true)?;
        let unavailable =
            Self::int_or_string_to_value(max_unavailable, name, "max_unavailable", desired, false)?;

        // Validation should never allow zero values for both max_surge and max_unavailable.
        // If both resolve to zero, set unavailable to 1 to ensure proper functionality.
        if surge == 0 && unavailable == 0 {
            return Ok(1);
        }

        Ok(unavailable)
    }

    /// Converts an IntOrString to its scaled value based on the total desired pods.
    fn int_or_string_to_value(
        int_or_string: Option<&IntOrString>,
        name: &str,
        field: &str,
        desired: i32,
        round_up: bool,
    ) -> Result<i32, HealthCheckerError> {
        match int_or_string {
            Some(value) => {
                let int_or_percentage =
                    IntOrPercentage::try_from(value.clone()).map_err(|err| {
                        HealthCheckerError::InvalidK8sObject {
                            kind: DEPLOYMENT_KIND.to_string(),
                            name: name.to_string(),
                            err: format!("Invalid IntOrString value: {}", err),
                        }
                    })?;

                Ok(int_or_percentage.scaled_value(desired, round_up))
            }
            None => Err(K8sHealthDeployment::missing_field_error(name, field)),
        }
    }

    // Returns the latest replica_set owned by the deployment whose name has been provided as parameter.
    /// In Kubernetes, it is possible to have multiple ReplicaSets for a single Deployment,
    /// especially during rollouts and updates. This function retrieves all ReplicaSets
    /// associated with the specified Deployment and returns the newest one based on the
    /// creation timestamp.
    fn latest_replica_set_for_deployment(&self, deployment_name: &str) -> Option<Arc<ReplicaSet>> {
        // Filter the list of ReplicaSets referencing to the deployment
        let mut replica_sets: Vec<Arc<ReplicaSet>> = self
            .k8s_client
            .list_replica_set()
            .into_iter()
            .filter(|rs| match &rs.metadata.owner_references {
                Some(owner_refereces) => owner_refereces
                    .iter()
                    .any(|owner| owner.kind == DEPLOYMENT_KIND && owner.name == deployment_name),
                None => false,
            })
            .collect();

        if replica_sets.is_empty() {
            return None;
        }

        // Sort ReplicaSets by creation timestamp in descending order and select the newest one.
        replica_sets.sort_by(|a, b| {
            b.metadata
                .creation_timestamp
                .cmp(&a.metadata.creation_timestamp)
        });
        replica_sets.into_iter().nth(0) // replica_sets.first() would return a reference
    }
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use super::*;
    use crate::{
        k8s::client::MockSyncK8sClient, sub_agent::health::k8s::health_checker::LABEL_RELEASE_FLUX,
    };
    use chrono::{DateTime, Utc};
    use k8s_openapi::api::apps::v1::{
        Deployment, DeploymentSpec, DeploymentStatus, DeploymentStrategy, ReplicaSetStatus,
        RollingUpdateDeployment,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference, Time};

    #[test]
    fn test_deployment_check_health() {
        struct TestCase {
            name: &'static str,
            deployment: Deployment,
            rs: Option<ReplicaSet>,
            expected: Health,
        }

        impl TestCase {
            fn run(self) {
                let result = if let Some(rs) = self.rs {
                    K8sHealthDeployment::check_deployment_health(
                        Arc::new(self.deployment),
                        Arc::new(rs),
                    )
                    .inspect_err(|err| {
                        panic!("Unexpected error getting health: {} - {}", err, self.name);
                    })
                    .unwrap()
                } else {
                    Health::unhealthy_with_last_error(format!(
                        "ReplicaSet not found for Deployment '{}'",
                        self.deployment
                            .metadata
                            .name
                            .as_deref()
                            .unwrap_or("unknown"),
                    ))
                };

                assert_eq!(result, self.expected, "{}", self.name);
            }
        }

        let test_cases = [
            TestCase {
                name: "Deployment ready",
                deployment: Deployment {
                    metadata: create_metadata("test-deployment"),
                    spec: Some(create_deployment_spec(10, "30%", "20%")),
                    status: Some(create_deployment_status(10)),
                },
                rs: Some(ReplicaSet {
                    metadata: create_metadata("test-rs"),
                    status: Some(create_replica_set_status(8)),
                    ..Default::default()
                }),
                expected: Healthy::default().into(),
            },
            TestCase {
                name: "Deployment not ready",
                deployment: Deployment {
                    metadata: create_metadata("test-deployment"),
                    spec: Some(create_deployment_spec(10, "30%", "20%")),
                    status: Some(create_deployment_status(10)),
                },
                rs: Some(ReplicaSet {
                    metadata: create_metadata("test-rs"),
                    status: Some(create_replica_set_status(6)),
                    ..Default::default()
                }),
                expected: Health::unhealthy_with_last_error(
                    "Deployment 'test-deployment' is not ready. 6 out of 8 expected pods are ready"
                        .into(),
                ),
            },
            TestCase {
                name: "Deployment paused",
                deployment: Deployment {
                    metadata: create_metadata("test-deployment"),
                    spec: Some(DeploymentSpec {
                        paused: Some(true),
                        ..create_deployment_spec(10, "30%", "20%")
                    }),
                    status: Some(create_deployment_status(10)),
                },
                rs: Some(ReplicaSet {
                    metadata: create_metadata("test-rs"),
                    status: Some(create_replica_set_status(8)),
                    ..Default::default()
                }),
                expected: Health::unhealthy_with_last_error(
                    "Deployment 'test-deployment' is paused".into(),
                ),
            },
            TestCase {
                name: "No ReplicaSet found",
                deployment: Deployment {
                    metadata: create_metadata("test-deployment"),
                    spec: Some(create_deployment_spec(10, "30%", "20%")),
                    status: Some(create_deployment_status(10)),
                },
                rs: None,
                expected: Health::unhealthy_with_last_error(
                    "ReplicaSet not found for Deployment 'test-deployment'".into(),
                ),
            },
        ];

        test_cases.into_iter().for_each(|tc| tc.run());
    }

    #[test]
    fn test_deployment_check_health_errors() {
        struct TestCase {
            name: &'static str,
            deployment: Deployment,
            rs: ReplicaSet,
            expected_err: HealthCheckerError,
        }

        impl TestCase {
            fn run(self) {
                let err = K8sHealthDeployment::check_deployment_health(
                    Arc::new(self.deployment),
                    Arc::new(self.rs),
                )
                .inspect(|result| {
                    panic!("Expected error, got {:?} for test - {}", result, self.name)
                })
                .unwrap_err();
                assert_eq!(
                    err.to_string(),
                    self.expected_err.to_string(),
                    "{} failed",
                    self.name
                );
            }
        }

        let test_cases = [
            TestCase {
                name: "Deployment without metadata.name",
                deployment: Deployment {
                    metadata: ObjectMeta {
                        name: None,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                rs: ReplicaSet {
                    metadata: create_metadata("test-rs"),
                    ..Default::default()
                },
                expected_err: HealthCheckerError::Generic(
                    "k8s error: Deployment does not have .metadata.name".to_string(),
                ),
            },
            TestCase {
                name: "Deployment without status",
                deployment: Deployment {
                    metadata: create_metadata("test-deployment"),
                    spec: Some(create_deployment_spec(10, "30%", "20%")),
                    status: None,
                },
                rs: ReplicaSet {
                    metadata: create_metadata("test-rs"),
                    ..Default::default()
                },
                expected_err: K8sHealthDeployment::missing_field_error("test-deployment", "status"),
            },
            TestCase {
                name: "Deployment without spec",
                deployment: Deployment {
                    metadata: create_metadata("test-deployment"),
                    spec: None,
                    status: Some(create_deployment_status(10)),
                },
                rs: ReplicaSet {
                    metadata: create_metadata("test-rs"),
                    ..Default::default()
                },
                expected_err: K8sHealthDeployment::missing_field_error("test-deployment", "spec"),
            },
            TestCase {
                name: "Deployment without status.replicas",
                deployment: Deployment {
                    metadata: create_metadata("test-deployment"),
                    spec: Some(create_deployment_spec(10, "30%", "20%")),
                    status: Some(DeploymentStatus {
                        replicas: None,
                        ..Default::default()
                    }),
                },
                rs: ReplicaSet {
                    metadata: create_metadata("test-rs"),
                    ..Default::default()
                },
                expected_err: K8sHealthDeployment::missing_field_error(
                    "test-deployment",
                    "status.replicas",
                ),
            },
            TestCase {
                name: "ReplicaSet without status",
                deployment: Deployment {
                    metadata: create_metadata("test-deployment"),
                    spec: Some(create_deployment_spec(10, "30%", "20%")),
                    status: Some(create_deployment_status(10)),
                },
                rs: ReplicaSet {
                    metadata: create_metadata("test-rs"),
                    status: None,
                    ..Default::default()
                },
                expected_err: K8sHealthDeployment::missing_field_error(
                    "test-deployment",
                    "replica set status",
                ),
            },
            TestCase {
                name: "ReplicaSet without status.ready_replicas",
                deployment: Deployment {
                    metadata: create_metadata("test-deployment"),
                    spec: Some(create_deployment_spec(10, "30%", "20%")),
                    status: Some(create_deployment_status(10)),
                },
                rs: ReplicaSet {
                    metadata: create_metadata("test-rs"),
                    status: Some(ReplicaSetStatus {
                        ready_replicas: None,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                expected_err: K8sHealthDeployment::missing_field_error(
                    "test-deployment",
                    "ready replicas",
                ),
            },
        ];

        test_cases.into_iter().for_each(|tc| tc.run());
    }

    #[test]
    fn test_latest_replica_for_deployment() {
        const DEPLOYMENT_NAME: &str = "deployment-name";

        struct TestCase {
            name: &'static str,
            replica_sets: Vec<Arc<ReplicaSet>>,
            expected_rs_name: Option<String>,
        }

        impl TestCase {
            fn run(self) {
                let mut k8s_client = MockSyncK8sClient::new();
                let (name, replica_sets, expected) =
                    (self.name, self.replica_sets, self.expected_rs_name);
                k8s_client
                    .expect_list_replica_set()
                    .times(1)
                    .returning(move || replica_sets.clone());

                let health_checker = K8sHealthDeployment {
                    k8s_client: Arc::new(k8s_client),
                    release_name: "release-name".to_string(),
                };

                let result = health_checker.latest_replica_set_for_deployment(DEPLOYMENT_NAME);
                assert_eq!(
                    result.map(|rs| rs.metadata.clone().name.unwrap()),
                    expected,
                    "Error in TestCase '{name}'"
                );
            }
        }

        let test_cases = [
            TestCase {
                name: "No replica-sets",
                replica_sets: Vec::new(),
                expected_rs_name: None,
            },
            TestCase {
                name: "No matching replica-set",
                replica_sets: vec![
                    Arc::new(create_replica_set(
                        "no-matching-kind",
                        "no-deployment-kind",
                        DEPLOYMENT_NAME,
                        None,
                    )),
                    Arc::new(create_replica_set(
                        "no-matching-name",
                        DEPLOYMENT_KIND,
                        "no-matching-name",
                        None,
                    )),
                    Arc::new(ReplicaSet::default()),
                ],
                expected_rs_name: None,
            },
            TestCase {
                name: "Only one matching",
                replica_sets: vec![
                    Arc::new(create_replica_set(
                        "no-matching-name",
                        DEPLOYMENT_KIND,
                        "no-matching-name",
                        None,
                    )),
                    Arc::new(create_replica_set(
                        "matching",
                        DEPLOYMENT_KIND,
                        DEPLOYMENT_NAME,
                        None,
                    )),
                ],
                expected_rs_name: Some("matching".to_string()),
            },
            TestCase {
                name: "Matching latest",
                replica_sets: vec![
                    Arc::new(create_replica_set(
                        "matching-1",
                        DEPLOYMENT_KIND,
                        DEPLOYMENT_NAME,
                        Some(Time(
                            DateTime::<Utc>::from_str("2024-05-27 09:00:00 +00:00").unwrap(),
                        )),
                    )),
                    Arc::new(create_replica_set(
                        "no-matching-name",
                        DEPLOYMENT_KIND,
                        "no-matching-name",
                        None,
                    )),
                    Arc::new(create_replica_set(
                        "matching-2",
                        DEPLOYMENT_KIND,
                        DEPLOYMENT_NAME,
                        Some(Time(
                            DateTime::<Utc>::from_str("2024-05-27 10:00:00 +00:00").unwrap(),
                        )),
                    )),
                    Arc::new(create_replica_set(
                        "matching-3",
                        DEPLOYMENT_KIND,
                        DEPLOYMENT_NAME,
                        Some(Time(
                            DateTime::<Utc>::from_str("2024-05-27 09:30:00 +00:00").unwrap(),
                        )),
                    )),
                ],
                expected_rs_name: Some("matching-2".to_string()),
            },
        ];
        test_cases.into_iter().for_each(|ts| ts.run())
    }

    #[test]
    fn test_max_unavailable() {
        struct TestCase {
            name: &'static str,
            deployment: Deployment,
            expected: Result<i32, HealthCheckerError>,
        }

        impl TestCase {
            fn run(self) {
                let metadata_name = self
                    .deployment
                    .metadata
                    .name
                    .as_deref()
                    .unwrap_or("unknown");
                let spec = self.deployment.spec.as_ref().unwrap();
                let result = K8sHealthDeployment::max_unavailable(metadata_name, spec);
                assert!(
                    match (&result, &self.expected) {
                        (Ok(r), Ok(e)) => r == e,
                        (Err(r), Err(e)) => r.to_string() == e.to_string(),
                        _ => false,
                    },
                    "{}",
                    self.name,
                );
            }
        }

        let test_cases = [
            TestCase {
                name: "MaxUnavailable as percentage",
                deployment: Deployment {
                    metadata: create_metadata("test-deployment"),
                    spec: Some(create_deployment_spec(10, "30%", "20%")),
                    ..Default::default()
                },
                expected: Ok(2),
            },
            TestCase {
                name: "MaxUnavailable as integer",
                deployment: Deployment {
                    metadata: create_metadata("test-deployment"),
                    spec: Some(create_deployment_spec(10, "3", "2")),
                    ..Default::default()
                },
                expected: Ok(2),
            },
            TestCase {
                name: "No replicas specified",
                deployment: Deployment {
                    metadata: create_metadata("test-deployment"),
                    spec: Some(DeploymentSpec {
                        replicas: None,
                        strategy: Some(DeploymentStrategy {
                            type_: Some(ROLLING_UPDATE.to_string()),
                            rolling_update: Some(RollingUpdateDeployment {
                                max_surge: Some(IntOrString::String("30%".to_string())),
                                max_unavailable: Some(IntOrString::String("20%".to_string())),
                            }),
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                expected: Err(HealthCheckerError::MissingK8sObjectField {
                    kind: "Deployment".to_string(),
                    name: "test-deployment".to_string(),
                    field: "spec.replicas".to_string(),
                }),
            },
            TestCase {
                name: "MaxUnavailable greater than replicas",
                deployment: Deployment {
                    metadata: create_metadata("test-deployment"),
                    spec: Some(create_deployment_spec(2, "30%", "100%")),
                    ..Default::default()
                },
                expected: Ok(2),
            },
            TestCase {
                name: "Invalid MaxSurge",
                deployment: Deployment {
                    metadata: create_metadata("test-deployment"),
                    spec: Some(DeploymentSpec {
                        replicas: Some(10),
                        strategy: Some(DeploymentStrategy {
                            type_: Some(ROLLING_UPDATE.to_string()),
                            rolling_update: Some(RollingUpdateDeployment {
                                max_surge: Some(IntOrString::String("invalid".to_string())),
                                max_unavailable: Some(IntOrString::String("20%".to_string())),
                            }),
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                expected: Err(HealthCheckerError::InvalidK8sObject {
                    kind: DEPLOYMENT_KIND.to_string(),
                    name: "test-deployment".to_string(),
                    err: "Invalid IntOrString value: invalid digit found in string".to_string(),
                }),
            },
            TestCase {
                name: "Non-rolling update strategy",
                deployment: Deployment {
                    metadata: create_metadata("test-deployment"),
                    spec: Some(DeploymentSpec {
                        replicas: Some(10),
                        strategy: Some(DeploymentStrategy {
                            type_: Some("Recreate".to_string()),
                            rolling_update: None,
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                expected: Ok(0),
            },
        ];

        test_cases.into_iter().for_each(|tc| tc.run());
    }

    #[test]
    fn test_health_check() {
        let release_name = "flux-release";
        let mut k8s_client = MockSyncK8sClient::new();

        let healthy_deployment_matching = Deployment {
            metadata: ObjectMeta {
                labels: Some([(LABEL_RELEASE_FLUX.to_string(), release_name.to_string())].into()),
                ..create_metadata("test-deployment")
            },
            spec: Some(create_deployment_spec(10, "30%", "20%")),
            status: Some(create_deployment_status(10)),
        };

        let deployment_with_no_replica_set = Deployment {
            metadata: ObjectMeta {
                labels: Some([(LABEL_RELEASE_FLUX.to_string(), release_name.to_string())].into()),
                ..create_metadata("test-deployment-2")
            },
            spec: Some(create_deployment_spec(10, "30%", "20%")),
            status: Some(create_deployment_status(10)),
        };

        let replica_sets = vec![Arc::new(ReplicaSet {
            status: Some(create_replica_set_status(8)),
            ..create_replica_set(
                "rs",
                DEPLOYMENT_KIND,
                "test-deployment",
                Some(Time(
                    DateTime::<Utc>::from_str("2024-05-27 10:00:00 +00:00").unwrap(),
                )),
            )
        })];

        k8s_client
            .expect_list_deployment()
            .times(1)
            .returning(move || {
                vec![
                    Arc::new(healthy_deployment_matching.clone()),
                    Arc::new(deployment_with_no_replica_set.clone()),
                ]
            });
        k8s_client
            .expect_list_replica_set()
            .times(2)
            .returning(move || replica_sets.clone());

        let health_checker =
            K8sHealthDeployment::new(Arc::new(k8s_client), release_name.to_string());
        let result = health_checker.check_health().unwrap();
        assert_eq!(
            result,
            Health::unhealthy_with_last_error(
                "ReplicaSet not found for Deployment test-deployment-2".to_string()
            )
        );
    }

    fn create_metadata(name: &str) -> ObjectMeta {
        ObjectMeta {
            name: Some(name.to_string()),
            ..Default::default()
        }
    }

    fn create_deployment_spec(
        replicas: i32,
        max_surge: &str,
        max_unavailable: &str,
    ) -> DeploymentSpec {
        DeploymentSpec {
            replicas: Some(replicas),
            strategy: Some(DeploymentStrategy {
                type_: Some(ROLLING_UPDATE.to_string()),
                rolling_update: Some(RollingUpdateDeployment {
                    max_surge: Some(IntOrString::String(max_surge.to_string())),
                    max_unavailable: Some(IntOrString::String(max_unavailable.to_string())),
                }),
            }),
            ..Default::default()
        }
    }

    fn create_deployment_status(replicas: i32) -> DeploymentStatus {
        DeploymentStatus {
            replicas: Some(replicas),
            ..Default::default()
        }
    }

    fn create_replica_set_status(ready_replicas: i32) -> ReplicaSetStatus {
        ReplicaSetStatus {
            ready_replicas: Some(ready_replicas),
            ..Default::default()
        }
    }

    fn create_replica_set(
        name: &str,
        owner_kind: &str,
        owner_name: &str,
        creation_timestamp: Option<Time>,
    ) -> ReplicaSet {
        ReplicaSet {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                owner_references: Some(vec![OwnerReference {
                    kind: owner_kind.to_string(),
                    name: owner_name.to_string(),
                    ..Default::default()
                }]),
                creation_timestamp,
                ..Default::default()
            },
            ..Default::default()
        }
    }
}
