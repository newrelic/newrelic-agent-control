use super::{
    ResourceFilter, check_health_for_items, flux_release_filter, missing_field_error, name_filter,
};
use crate::checkers::health::health_checker::{
    Health, HealthChecker, HealthCheckerError, Healthy, Unhealthy,
};
use crate::checkers::health::with_start_time::{HealthWithStartTime, StartTime};
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::k8s::utils as client_utils;
use k8s_openapi::api::apps::v1::Deployment;
use std::sync::Arc;

#[derive(Debug)]
pub struct K8sHealthDeployment {
    k8s_client: Arc<SyncK8sClient>,
    filter: ResourceFilter,
    start_time: StartTime,
    namespace: String,
}

impl HealthChecker for K8sHealthDeployment {
    fn check_health(&self) -> Result<HealthWithStartTime, HealthCheckerError> {
        let deployments = self.k8s_client.list_deployment(&self.namespace)?;

        let health = match &self.filter {
            ResourceFilter::ByName(name) => check_health_for_items(
                deployments.into_iter().filter(name_filter(name.clone())),
                Self::check_deployment_health,
            )?,
            ResourceFilter::ByFluxLabel(release) => check_health_for_items(
                deployments
                    .into_iter()
                    .filter(flux_release_filter(release.clone())),
                Self::check_deployment_health,
            )?,
        };

        Ok(HealthWithStartTime::new(health, self.start_time))
    }
}

impl K8sHealthDeployment {
    pub fn new(
        k8s_client: Arc<SyncK8sClient>,
        filter: ResourceFilter,
        start_time: StartTime,
        namespace: String,
    ) -> Self {
        Self {
            k8s_client,
            filter,
            start_time,
            namespace,
        }
    }

    /// Checks the health of a specific Deployment.
    pub fn check_deployment_health(deployment: &Deployment) -> Result<Health, HealthCheckerError> {
        let name = client_utils::get_metadata_name(deployment)?;

        let status = deployment
            .status
            .as_ref()
            .ok_or_else(|| missing_field_error(deployment, name.as_str(), ".status"))?;

        // The deployment is unhealthy if any of the pods are unavailable, i.e. not running or not ready.
        if let Some(unavailable_replicas) = status.unavailable_replicas
            && unavailable_replicas > 0
        {
            return Ok(Unhealthy::new(format!(
                "Deployment `{name}`: has {unavailable_replicas} unavailable replicas"
            ))
            .into());
        };

        let desired_replicas = deployment
            .spec
            .as_ref()
            .ok_or_else(|| missing_field_error(deployment, name.as_str(), ".spec"))?
            .replicas
            .ok_or_else(|| missing_field_error(deployment, &name, "spec.replicas"))?;

        // This condition is more of a safe net, as if there are no unavailable replicas, available replicas should match desired replicas.
        // available_replicas is present only if > 0
        let available_replicas = status.available_replicas.unwrap_or_default();
        if available_replicas < desired_replicas {
            return Ok(Unhealthy::new(
                    format!("Deployment `{name}`: available replicas `{available_replicas}` is less than desired `{desired_replicas}`"),
                )
                .into());
        }

        Ok(Healthy::new().into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkers::health::health_checker::Healthy;
    use crate::checkers::health::k8s::health_checker::resources::daemon_set::tests::TEST_NAMESPACE;
    use crate::k8s::client::MockSyncK8sClient;
    use k8s_openapi::api::apps::v1::{
        Deployment, DeploymentSpec, DeploymentStatus, DeploymentStrategy, RollingUpdateDeployment,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;

    #[test]
    fn test_deployment_check_health() {
        struct TestCase {
            name: &'static str,
            deployment: Deployment,
            expected: Health,
        }

        impl TestCase {
            fn run(self) {
                let result = K8sHealthDeployment::check_deployment_health(&self.deployment)
                    .inspect_err(|err| {
                        panic!("Unexpected error getting health: {} - {}", err, self.name);
                    })
                    .unwrap();

                assert_eq!(result, self.expected, "{}", self.name);
            }
        }

        let test_cases = [
            // Healthy cases
            TestCase {
                name: "Deployment with zero replicas",
                deployment: Deployment {
                    metadata: test_util_create_metadata("test-deployment"),
                    spec: Some(DeploymentSpec {
                        replicas: Some(0),
                        ..Default::default()
                    }),
                    status: Some(DeploymentStatus {
                        ..Default::default()
                    }),
                },
                expected: Healthy::new().into(),
            },
            TestCase {
                name: "Deployment with zero replicas, zero values",
                deployment: Deployment {
                    metadata: test_util_create_metadata("test-deployment"),
                    spec: Some(DeploymentSpec {
                        replicas: Some(0),
                        ..Default::default()
                    }),
                    status: Some(DeploymentStatus {
                        available_replicas: Some(0),
                        unavailable_replicas: Some(0),
                        ..Default::default()
                    }),
                },
                expected: Healthy::new().into(),
            },
            TestCase {
                name: "Deployment with replicas",
                deployment: Deployment {
                    metadata: test_util_create_metadata("test-deployment"),
                    spec: Some(DeploymentSpec {
                        replicas: Some(10),
                        ..Default::default()
                    }),
                    status: Some(DeploymentStatus {
                        available_replicas: Some(10),
                        ..Default::default()
                    }),
                },
                expected: Healthy::new().into(),
            },
            // Unhealthy cases
            TestCase {
                name: "Deployment with unavailable replicas",
                deployment: Deployment {
                    metadata: test_util_create_metadata("test-deployment"),
                    spec: Some(DeploymentSpec {
                        replicas: Some(1),
                        ..Default::default()
                    }),
                    status: Some(DeploymentStatus {
                        available_replicas: Some(1),
                        unavailable_replicas: Some(1),
                        ..Default::default()
                    }),
                },
                expected: Unhealthy::new(
                    "Deployment `test-deployment`: has 1 unavailable replicas".into(),
                )
                .into(),
            },
            TestCase {
                name: "Deployment with desired replicas not matching available replicas",
                deployment: Deployment {
                    metadata: test_util_create_metadata("test-deployment"),
                    spec: Some(DeploymentSpec {
                        replicas: Some(10),
                        ..Default::default()
                    }),
                    status: Some(DeploymentStatus {
                        available_replicas: Some(9),
                        unavailable_replicas: None,
                        ..Default::default()
                    }),
                },
                expected: Unhealthy::new(
                    "Deployment `test-deployment`: available replicas `9` is less than desired `10`"
                        .into(),
                )
                .into(),
            },
            TestCase {
                name: "Deployment with desired replicas not matching available replicas (None)",
                deployment: Deployment {
                    metadata: test_util_create_metadata("test-deployment"),
                    spec: Some(DeploymentSpec {
                        replicas: Some(10),
                        ..Default::default()
                    }),
                    status: Some(DeploymentStatus {
                        available_replicas: None,
                        unavailable_replicas: None,
                        ..Default::default()
                    }),
                },
                expected: Unhealthy::new(
                    "Deployment `test-deployment`: available replicas `0` is less than desired `10`"
                        .into(),
                )
                .into(),
            },
        ];

        test_cases.into_iter().for_each(|tc| tc.run());
    }

    #[test]
    fn test_deployment_check_health_errors() {
        struct TestCase {
            name: &'static str,
            deployment: Deployment,
            expected_err: HealthCheckerError,
        }

        impl TestCase {
            fn run(self) {
                let err = K8sHealthDeployment::check_deployment_health(&self.deployment)
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
                expected_err: missing_field_error(
                    &Deployment::default(),
                    "test-deployment",
                    ".status",
                ),
            },
            TestCase {
                name: "Deployment without spec",
                deployment: Deployment {
                    metadata: test_util_create_metadata("test-deployment"),
                    spec: None,
                    status: Some(test_util_create_deployment_status(10)),
                },
                expected_err: missing_field_error(
                    &Deployment::default(),
                    "test-deployment",
                    ".spec",
                ),
            },
            TestCase {
                name: "Deployment without spec.replicas",
                deployment: Deployment {
                    metadata: test_util_create_metadata("test-deployment"),
                    spec: Some(DeploymentSpec {
                        ..Default::default()
                    }),
                    status: Some(DeploymentStatus {
                        ..Default::default()
                    }),
                },
                expected_err: missing_field_error(
                    &Deployment::default(),
                    "test-deployment",
                    "spec.replicas",
                ),
            },
        ];

        test_cases.into_iter().for_each(|tc| tc.run());
    }

    #[test]
    fn test_check_health() {
        struct TestCase {
            name: &'static str,
            deployments: Vec<Arc<Deployment>>,
            expected_health: Health,
        }

        impl TestCase {
            fn run(self) {
                let mut k8s_client = MockSyncK8sClient::new();
                k8s_client
                    .expect_list_deployment()
                    .times(1)
                    .returning(move |_| Ok(self.deployments.clone()));

                let start_time = StartTime::now();
                let health_checker = K8sHealthDeployment::new(
                    Arc::new(k8s_client),
                    ResourceFilter::ByName("target-deployment".to_string()),
                    start_time,
                    TEST_NAMESPACE.to_string(),
                );
                let health = health_checker.check_health().unwrap_or_else(|_| {
                    panic!("Unexpected error getting health for test - {}", self.name)
                });
                assert_eq!(
                    health,
                    HealthWithStartTime::new(self.expected_health, start_time),
                    "{} failed",
                    self.name
                );
            }
        }

        let matching_healthy = Deployment {
            metadata: test_util_create_metadata("target-deployment"),
            spec: Some(test_util_create_deployment_spec(10, "30%", "20%")),
            status: Some(test_util_create_deployment_status(10)),
        };
        let matching_unhealthy = Deployment {
            metadata: test_util_create_metadata("target-deployment"),
            spec: Some(test_util_create_deployment_spec(10, "30%", "20%")),
            status: Some(DeploymentStatus {
                available_replicas: Some(9),
                unavailable_replicas: Some(1),
                ..Default::default()
            }),
        };
        // Would fail if checked — must be skipped because name doesn't match.
        let non_matching = Deployment {
            metadata: test_util_create_metadata("other-deployment"),
            ..Default::default()
        };

        let test_cases = [
            TestCase {
                name: "Matching healthy deployment",
                deployments: vec![Arc::new(matching_healthy.clone())],
                expected_health: Healthy::new().into(),
            },
            TestCase {
                name: "Matching unhealthy deployment",
                deployments: vec![Arc::new(matching_unhealthy.clone())],
                expected_health: Unhealthy::new(
                    "Deployment `target-deployment`: has 1 unavailable replicas".into(),
                )
                .into(),
            },
            TestCase {
                name: "Non-matching deployment is skipped",
                deployments: vec![Arc::new(non_matching.clone())],
                expected_health: Healthy::new().into(),
            },
        ];

        test_cases.into_iter().for_each(|tc| tc.run());
    }

    #[test]
    fn test_check_health_for_helm_release() {
        use crate::checkers::health::k8s::health_checker::LABEL_RELEASE_FLUX;

        struct TestCase {
            name: &'static str,
            deployments: Vec<Arc<Deployment>>,
            expected_health: Health,
        }

        impl TestCase {
            fn run(self) {
                let mut k8s_client = MockSyncK8sClient::new();
                k8s_client
                    .expect_list_deployment()
                    .times(1)
                    .returning(move |_| Ok(self.deployments.clone()));

                let start_time = StartTime::now();
                let health_checker = K8sHealthDeployment::new(
                    Arc::new(k8s_client),
                    ResourceFilter::ByFluxLabel("flux-release".to_string()),
                    start_time,
                    TEST_NAMESPACE.to_string(),
                );
                let health = health_checker.check_health().unwrap_or_else(|_| {
                    panic!("Unexpected error getting health for test - {}", self.name)
                });
                assert_eq!(
                    health,
                    HealthWithStartTime::new(self.expected_health, start_time),
                    "{} failed",
                    self.name
                );
            }
        }

        let flux_label =
            || Some([(LABEL_RELEASE_FLUX.to_string(), "flux-release".to_string())].into());

        let healthy = Deployment {
            metadata: ObjectMeta {
                labels: flux_label(),
                ..test_util_create_metadata("chart-deployment-a")
            },
            spec: Some(test_util_create_deployment_spec(10, "30%", "20%")),
            status: Some(test_util_create_deployment_status(10)),
        };
        let unhealthy_foo = Deployment {
            metadata: ObjectMeta {
                labels: flux_label(),
                ..test_util_create_metadata("chart-deployment-foo")
            },
            spec: Some(test_util_create_deployment_spec(10, "30%", "20%")),
            status: Some(DeploymentStatus {
                available_replicas: Some(9),
                unavailable_replicas: Some(1),
                ..Default::default()
            }),
        };
        let unhealthy_bar = Deployment {
            metadata: ObjectMeta {
                labels: flux_label(),
                ..test_util_create_metadata("chart-deployment-bar")
            },
            spec: Some(test_util_create_deployment_spec(10, "30%", "20%")),
            status: Some(DeploymentStatus {
                available_replicas: Some(9),
                unavailable_replicas: Some(1),
                ..Default::default()
            }),
        };
        // Does not carry the Flux label — must be skipped.
        let non_matching = Deployment {
            metadata: test_util_create_metadata("other-deployment"),
            ..Default::default()
        };

        let test_cases = [
            TestCase {
                name: "Healthy deployments",
                deployments: vec![Arc::new(healthy.clone()), Arc::new(healthy.clone())],
                expected_health: Healthy::new().into(),
            },
            TestCase {
                name: "Non-matching deployment is skipped",
                deployments: vec![Arc::new(non_matching.clone()), Arc::new(healthy.clone())],
                expected_health: Healthy::new().into(),
            },
            TestCase {
                name: "First unhealthy reported (foo before bar)",
                deployments: vec![
                    Arc::new(healthy.clone()),
                    Arc::new(unhealthy_foo.clone()),
                    Arc::new(unhealthy_bar.clone()),
                ],
                expected_health: Unhealthy::new(
                    "Deployment `chart-deployment-foo`: has 1 unavailable replicas".into(),
                )
                .into(),
            },
            TestCase {
                name: "First unhealthy reported (bar before foo)",
                deployments: vec![
                    Arc::new(healthy.clone()),
                    Arc::new(unhealthy_bar.clone()),
                    Arc::new(unhealthy_foo.clone()),
                ],
                expected_health: Unhealthy::new(
                    "Deployment `chart-deployment-bar`: has 1 unavailable replicas".into(),
                )
                .into(),
            },
        ];

        test_cases.into_iter().for_each(|tc| tc.run());
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
                type_: Some("RollingUpdate".to_string()),
                rolling_update: Some(RollingUpdateDeployment {
                    max_surge: Some(IntOrString::String(max_surge.to_string())),
                    max_unavailable: Some(IntOrString::String(max_unavailable.to_string())),
                }),
            }),
            ..Default::default()
        }
    }

    fn test_util_create_deployment_status(available_replicas: i32) -> DeploymentStatus {
        DeploymentStatus {
            available_replicas: Some(available_replicas),
            ..Default::default()
        }
    }
}
