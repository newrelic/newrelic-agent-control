#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::k8s::utils::IntOrPercentage;
use crate::sub_agent::health::health_checker::{Health, HealthChecker, HealthCheckerError};
use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec, ReplicaSet};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use std::sync::Arc;

use super::utils::{self, check_health_for_items, flux_release_filter};

const ROLLING_UPDATE: &str = "RollingUpdate";

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

        check_health_for_items(target_deployments, |arc_deployment: Arc<Deployment>| {
            let deployment: &Deployment = &arc_deployment; // Dereferencing the Arc so it is usable by generics.
            let name = utils::get_metadata_name(deployment)?;

            self.latest_replica_set_for_deployment(deployment, name.as_str())
                .map(|replica_set| Self::check_deployment_health(arc_deployment, replica_set))
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

    /// Checks the health of a specific deployment and its associated ReplicaSet.
    pub fn check_deployment_health(
        deployment: Arc<Deployment>,
        rs: Arc<ReplicaSet>,
    ) -> Result<Health, HealthCheckerError> {
        let name = utils::get_metadata_name(&*deployment)?;

        let status = deployment
            .status
            .as_ref()
            .ok_or_else(|| utils::missing_field_error(&*deployment, &name, "status"))?;

        let spec = deployment
            .spec
            .as_ref()
            .ok_or_else(|| utils::missing_field_error(&*deployment, &name, "spec"))?;

        // If the deployment is paused, consider it unhealthy
        if let Some(true) = spec.paused {
            return Ok(Health::unhealthy_with_last_error(format!(
                "Deployment '{}' is paused",
                name,
            )));
        }

        let replicas = status
            .replicas
            .ok_or_else(|| utils::missing_field_error(&*deployment, &name, "status.replicas"))?;

        let max_unavailable = Self::max_unavailable(&deployment, &name, spec)?;

        let expected_ready = replicas.checked_sub(max_unavailable).ok_or_else(|| {
            HealthCheckerError::Generic(format!(
                "Invalid calculation for expected ready replicas for Deployment '{}'",
                name
            ))
        })?;

        let rs_status = rs
            .status
            .as_ref()
            .ok_or_else(|| utils::missing_field_error(&*deployment, &name, "replica set status"))?;

        let ready_replicas = rs_status
            .ready_replicas
            .ok_or_else(|| utils::missing_field_error(&*deployment, &name, "ready replicas"))?;

        if ready_replicas < expected_ready {
            return Ok(Health::unhealthy_with_last_error(format!(
                "Deployment '{}' is not ready. {} out of {} expected pods are ready",
                name, ready_replicas, expected_ready
            )));
        }

        Ok(Health::healthy())
    }

    /// Calculates the maximum number of unavailable pods during a rolling update.
    fn max_unavailable(
        deployment: &Deployment,
        metadata_name: &str,
        spec: &DeploymentSpec,
    ) -> Result<i32, HealthCheckerError> {
        let replicas = spec.replicas.ok_or_else(|| {
            utils::missing_field_error(deployment, metadata_name, "spec.replicas")
        })?;

        if !Self::is_rolling_update(deployment, metadata_name, spec)? || replicas == 0 {
            return Ok(0);
        }

        let rolling_update_strategy = spec
            .strategy
            .as_ref()
            .and_then(|strategy| strategy.rolling_update.as_ref());
        let max_surge = rolling_update_strategy.and_then(|ru| ru.max_surge.as_ref());
        let max_unavailable = rolling_update_strategy.and_then(|ru| ru.max_unavailable.as_ref());

        let max_unavailable = Self::resolve_fenceposts(
            deployment,
            max_surge,
            max_unavailable,
            metadata_name,
            replicas,
        )?;

        Ok(max_unavailable.min(replicas))
    }

    /// Checks if the deployment strategy is a rolling update.
    fn is_rolling_update(
        deployment: &Deployment,
        metadata_name: &str,
        spec: &DeploymentSpec,
    ) -> Result<bool, HealthCheckerError> {
        let strategy = spec.strategy.as_ref().ok_or_else(|| {
            utils::missing_field_error(deployment, metadata_name, "spec.strategy")
        })?;

        let type_ = strategy.type_.as_deref().ok_or_else(|| {
            utils::missing_field_error(deployment, metadata_name, "spec.strategy.type")
        })?;

        Ok(type_ == ROLLING_UPDATE)
    }

    /// Return the maximum number of unavailable pods during a rolling update.
    ///
    /// Ensures the calculated value is within reasonable bounds and defaults to 1 if both
    /// max_surge and max_unavailable resolve to zero.
    fn resolve_fenceposts(
        deployment: &Deployment,
        max_surge: Option<&IntOrString>,
        max_unavailable: Option<&IntOrString>,
        name: &str,
        desired: i32,
    ) -> Result<i32, HealthCheckerError> {
        let surge =
            Self::int_or_string_to_value(deployment, max_surge, name, "max_surge", desired, true)?;
        let unavailable = Self::int_or_string_to_value(
            deployment,
            max_unavailable,
            name,
            "max_unavailable",
            desired,
            false,
        )?;

        // Validation should never allow zero values for both max_surge and max_unavailable.
        // If both resolve to zero, set unavailable to 1 to ensure proper functionality.
        if surge == 0 && unavailable == 0 {
            return Ok(1);
        }

        Ok(unavailable)
    }

    /// Converts an IntOrString to its scaled value based on the total desired pods.
    fn int_or_string_to_value(
        deployment: &Deployment,
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
                            kind: utils::get_kind(deployment).into(),
                            name: name.to_string(),
                            err: format!("Invalid IntOrString value: {}", err),
                        }
                    })?;

                Ok(int_or_percentage.scaled_value(desired, round_up))
            }
            None => Err(utils::missing_field_error(deployment, name, field)),
        }
    }

    // Returns the latest replica_set owned by the deployment whose name has been provided as parameter.
    /// In Kubernetes, it is possible to have multiple ReplicaSets for a single Deployment,
    /// especially during rollouts and updates. This function retrieves all ReplicaSets
    /// associated with the specified Deployment and returns the newest one based on the
    /// creation timestamp.
    fn latest_replica_set_for_deployment(
        &self,
        deployment: &Deployment,
        deployment_name: &str,
    ) -> Option<Arc<ReplicaSet>> {
        // Filter the list of ReplicaSets referencing to the deployment
        let mut replica_sets: Vec<Arc<ReplicaSet>> = self
            .k8s_client
            .list_replica_set()
            .into_iter()
            .filter(|rs| match &rs.metadata.owner_references {
                Some(owner_refereces) => owner_refereces.iter().any(|owner| {
                    owner.kind == utils::get_kind(deployment) && owner.name == deployment_name
                }),
                None => false,
            })
            .collect();

        // Sort ReplicaSets by creation timestamp in descending order and select the newest one.
        replica_sets.sort_by(|a, b| {
            b.metadata
                .creation_timestamp
                .cmp(&a.metadata.creation_timestamp)
        });

        replica_sets.into_iter().next() // replica_sets.first() would return a reference
    }
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use super::*;
    use crate::sub_agent::health::health_checker::Healthy;
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
                    metadata: test_util_create_metadata("test-deployment"),
                    spec: Some(test_util_create_deployment_spec(10, "30%", "20%")),
                    status: Some(test_util_create_deployment_status(10)),
                },
                rs: Some(ReplicaSet {
                    metadata: test_util_create_metadata("test-rs"),
                    status: Some(test_util_create_replica_set_status(8)),
                    ..Default::default()
                }),
                expected: Healthy::default().into(),
            },
            TestCase {
                name: "Deployment not ready",
                deployment: Deployment {
                    metadata: test_util_create_metadata("test-deployment"),
                    spec: Some(test_util_create_deployment_spec(10, "30%", "20%")),
                    status: Some(test_util_create_deployment_status(10)),
                },
                rs: Some(ReplicaSet {
                    metadata: test_util_create_metadata("test-rs"),
                    status: Some(test_util_create_replica_set_status(6)),
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
                    metadata: test_util_create_metadata("test-deployment"),
                    spec: Some(DeploymentSpec {
                        paused: Some(true),
                        ..test_util_create_deployment_spec(10, "30%", "20%")
                    }),
                    status: Some(test_util_create_deployment_status(10)),
                },
                rs: Some(ReplicaSet {
                    metadata: test_util_create_metadata("test-rs"),
                    status: Some(test_util_create_replica_set_status(8)),
                    ..Default::default()
                }),
                expected: Health::unhealthy_with_last_error(
                    "Deployment 'test-deployment' is paused".into(),
                ),
            },
            TestCase {
                name: "No ReplicaSet found",
                deployment: Deployment {
                    metadata: test_util_create_metadata("test-deployment"),
                    spec: Some(test_util_create_deployment_spec(10, "30%", "20%")),
                    status: Some(test_util_create_deployment_status(10)),
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
                    metadata: test_util_create_metadata("test-rs"),
                    ..Default::default()
                },
                expected_err: HealthCheckerError::Generic(
                    "k8s error: Deployment does not have .metadata.name".to_string(),
                ),
            },
            TestCase {
                name: "Deployment without status",
                deployment: Deployment {
                    metadata: test_util_create_metadata("test-deployment"),
                    spec: Some(test_util_create_deployment_spec(10, "30%", "20%")),
                    status: None,
                },
                rs: ReplicaSet {
                    metadata: test_util_create_metadata("test-rs"),
                    ..Default::default()
                },
                expected_err: utils::missing_field_error(
                    &test_util_get_empty_deployment(),
                    "test-deployment",
                    "status",
                ),
            },
            TestCase {
                name: "Deployment without spec",
                deployment: Deployment {
                    metadata: test_util_create_metadata("test-deployment"),
                    spec: None,
                    status: Some(test_util_create_deployment_status(10)),
                },
                rs: ReplicaSet {
                    metadata: test_util_create_metadata("test-rs"),
                    ..Default::default()
                },
                expected_err: utils::missing_field_error(
                    &test_util_get_empty_deployment(),
                    "test-deployment",
                    "spec",
                ),
            },
            TestCase {
                name: "Deployment without status.replicas",
                deployment: Deployment {
                    metadata: test_util_create_metadata("test-deployment"),
                    spec: Some(test_util_create_deployment_spec(10, "30%", "20%")),
                    status: Some(DeploymentStatus {
                        replicas: None,
                        ..Default::default()
                    }),
                },
                rs: ReplicaSet {
                    metadata: test_util_create_metadata("test-rs"),
                    ..Default::default()
                },
                expected_err: utils::missing_field_error(
                    &test_util_get_empty_deployment(),
                    "test-deployment",
                    "status.replicas",
                ),
            },
            TestCase {
                name: "ReplicaSet without status",
                deployment: Deployment {
                    metadata: test_util_create_metadata("test-deployment"),
                    spec: Some(test_util_create_deployment_spec(10, "30%", "20%")),
                    status: Some(test_util_create_deployment_status(10)),
                },
                rs: ReplicaSet {
                    metadata: test_util_create_metadata("test-rs"),
                    status: None,
                    ..Default::default()
                },
                expected_err: utils::missing_field_error(
                    &test_util_get_empty_deployment(),
                    "test-deployment",
                    "replica set status",
                ),
            },
            TestCase {
                name: "ReplicaSet without status.ready_replicas",
                deployment: Deployment {
                    metadata: test_util_create_metadata("test-deployment"),
                    spec: Some(test_util_create_deployment_spec(10, "30%", "20%")),
                    status: Some(test_util_create_deployment_status(10)),
                },
                rs: ReplicaSet {
                    metadata: test_util_create_metadata("test-rs"),
                    status: Some(ReplicaSetStatus {
                        ready_replicas: None,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                expected_err: utils::missing_field_error(
                    &test_util_get_empty_deployment(),
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

                let result = health_checker.latest_replica_set_for_deployment(
                    &test_util_get_empty_deployment(),
                    DEPLOYMENT_NAME,
                );
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
                    Arc::new(test_util_create_replica_set(
                        "no-matching-kind",
                        "no-deployment-kind".into(),
                        DEPLOYMENT_NAME,
                        None,
                    )),
                    Arc::new(test_util_create_replica_set(
                        "no-matching-name",
                        test_util_get_deployment_kind(),
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
                    Arc::new(test_util_create_replica_set(
                        "no-matching-name",
                        test_util_get_deployment_kind(),
                        "no-matching-name",
                        None,
                    )),
                    Arc::new(test_util_create_replica_set(
                        "matching",
                        test_util_get_deployment_kind(),
                        DEPLOYMENT_NAME,
                        None,
                    )),
                ],
                expected_rs_name: Some("matching".to_string()),
            },
            TestCase {
                name: "Matching latest",
                replica_sets: vec![
                    Arc::new(test_util_create_replica_set(
                        "matching-1",
                        test_util_get_deployment_kind(),
                        DEPLOYMENT_NAME,
                        Some(Time(
                            DateTime::<Utc>::from_str("2024-05-27 09:00:00 +00:00").unwrap(),
                        )),
                    )),
                    Arc::new(test_util_create_replica_set(
                        "no-matching-name",
                        test_util_get_deployment_kind(),
                        "no-matching-name",
                        None,
                    )),
                    Arc::new(test_util_create_replica_set(
                        "matching-2",
                        test_util_get_deployment_kind(),
                        DEPLOYMENT_NAME,
                        Some(Time(
                            DateTime::<Utc>::from_str("2024-05-27 10:00:00 +00:00").unwrap(),
                        )),
                    )),
                    Arc::new(test_util_create_replica_set(
                        "matching-3",
                        test_util_get_deployment_kind(),
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
                let result = K8sHealthDeployment::max_unavailable(
                    &test_util_get_empty_deployment(),
                    metadata_name,
                    spec,
                );
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
                    metadata: test_util_create_metadata("test-deployment"),
                    spec: Some(test_util_create_deployment_spec(10, "30%", "20%")),
                    ..Default::default()
                },
                expected: Ok(2),
            },
            TestCase {
                name: "MaxUnavailable as integer",
                deployment: Deployment {
                    metadata: test_util_create_metadata("test-deployment"),
                    spec: Some(test_util_create_deployment_spec(10, "3", "2")),
                    ..Default::default()
                },
                expected: Ok(2),
            },
            TestCase {
                name: "No replicas specified",
                deployment: Deployment {
                    metadata: test_util_create_metadata("test-deployment"),
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
                    metadata: test_util_create_metadata("test-deployment"),
                    spec: Some(test_util_create_deployment_spec(2, "30%", "100%")),
                    ..Default::default()
                },
                expected: Ok(2),
            },
            TestCase {
                name: "Invalid MaxSurge",
                deployment: Deployment {
                    metadata: test_util_create_metadata("test-deployment"),
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
                    kind: test_util_get_deployment_kind(),
                    name: "test-deployment".to_string(),
                    err: "Invalid IntOrString value: invalid digit found in string".to_string(),
                }),
            },
            TestCase {
                name: "Non-rolling update strategy",
                deployment: Deployment {
                    metadata: test_util_create_metadata("test-deployment"),
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
                ..test_util_create_metadata("test-deployment")
            },
            spec: Some(test_util_create_deployment_spec(10, "30%", "20%")),
            status: Some(test_util_create_deployment_status(10)),
        };

        let deployment_with_no_replica_set = Deployment {
            metadata: ObjectMeta {
                labels: Some([(LABEL_RELEASE_FLUX.to_string(), release_name.to_string())].into()),
                ..test_util_create_metadata("test-deployment-2")
            },
            spec: Some(test_util_create_deployment_spec(10, "30%", "20%")),
            status: Some(test_util_create_deployment_status(10)),
        };

        let replica_sets = vec![Arc::new(ReplicaSet {
            status: Some(test_util_create_replica_set_status(8)),
            ..test_util_create_replica_set(
                "rs",
                test_util_get_deployment_kind(),
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

    fn test_util_create_metadata(name: &str) -> ObjectMeta {
        ObjectMeta {
            name: Some(name.to_string()),
            ..Default::default()
        }
    }

    fn test_util_create_deployment_spec(
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

    fn test_util_create_deployment_status(replicas: i32) -> DeploymentStatus {
        DeploymentStatus {
            replicas: Some(replicas),
            ..Default::default()
        }
    }

    fn test_util_create_replica_set_status(ready_replicas: i32) -> ReplicaSetStatus {
        ReplicaSetStatus {
            ready_replicas: Some(ready_replicas),
            ..Default::default()
        }
    }

    fn test_util_create_replica_set(
        name: &str,
        owner_kind: String,
        owner_name: &str,
        creation_timestamp: Option<Time>,
    ) -> ReplicaSet {
        ReplicaSet {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                owner_references: Some(vec![OwnerReference {
                    kind: owner_kind,
                    name: owner_name.to_string(),
                    ..Default::default()
                }]),
                creation_timestamp,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn test_util_get_empty_deployment() -> Deployment {
        Deployment {
            ..Default::default()
        }
    }

    fn test_util_get_deployment_kind() -> String {
        utils::get_kind(&test_util_get_empty_deployment()).into()
    }
}
