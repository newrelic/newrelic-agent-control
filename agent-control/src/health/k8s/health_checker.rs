use crate::agent_control::config::{helmrelease_v2_type_meta, instrumentation_v1beta1_type_meta};
use crate::health::health_checker::{HealthChecker, HealthCheckerError, Healthy};
use crate::health::with_start_time::{HealthWithStartTime, StartTime};
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use kube::api::{DynamicObject, TypeMeta};
use resources::{
    daemon_set::K8sHealthDaemonSet, deployment::K8sHealthDeployment,
    helm_release::K8sHealthFluxHelmRelease, instrumentation::K8sHealthNRInstrumentation,
    stateful_set::K8sHealthStatefulSet,
};
use std::sync::Arc;
use tracing::trace;

mod resources;

// This label selector is added in post-render and present no matter the chart we are installing
// https://github.com/fluxcd/helm-controller/blob/main/CHANGELOG.md#090
pub const LABEL_RELEASE_FLUX: &str = "helm.toolkit.fluxcd.io/name";

/// This enum wraps all the health check implementations related to a Kubernetes resource.
#[derive(Debug)]
pub enum K8sResourceHealthChecker {
    Flux(K8sHealthFluxHelmRelease),
    NewRelic(K8sHealthNRInstrumentation),
    StatefulSet(K8sHealthStatefulSet),
    DaemonSet(K8sHealthDaemonSet),
    Deployment(K8sHealthDeployment),
}

impl HealthChecker for K8sResourceHealthChecker {
    fn check_health(&self) -> Result<HealthWithStartTime, HealthCheckerError> {
        match self {
            K8sResourceHealthChecker::Flux(flux) => flux.check_health(),
            K8sResourceHealthChecker::NewRelic(nr_instrumentation) => {
                nr_instrumentation.check_health()
            }
            K8sResourceHealthChecker::StatefulSet(stateful_set) => stateful_set.check_health(),
            K8sResourceHealthChecker::DaemonSet(daemon_set) => daemon_set.check_health(),
            K8sResourceHealthChecker::Deployment(deployment) => deployment.check_health(),
        }
    }
}

/// Returns the health-checks corresponding to a type_meta
pub fn health_checkers_for_type_meta(
    type_meta: TypeMeta,
    k8s_client: Arc<SyncK8sClient>,
    name: String,
    start_time: StartTime,
) -> Vec<K8sResourceHealthChecker> {
    // HelmRelease (Flux CR)
    if type_meta == helmrelease_v2_type_meta() {
        vec![
            K8sResourceHealthChecker::Flux(K8sHealthFluxHelmRelease::new(
                k8s_client.clone(),
                type_meta,
                name.clone(),
                start_time,
            )),
            K8sResourceHealthChecker::StatefulSet(K8sHealthStatefulSet::new(
                k8s_client.clone(),
                name.clone(),
                start_time,
            )),
            K8sResourceHealthChecker::DaemonSet(K8sHealthDaemonSet::new(
                k8s_client.clone(),
                name.clone(),
                start_time,
            )),
            K8sResourceHealthChecker::Deployment(K8sHealthDeployment::new(
                k8s_client.clone(),
                name,
                start_time,
            )),
        ]
    // Instrumentation (Newrelic CR)
    } else if type_meta == instrumentation_v1beta1_type_meta() {
        vec![K8sResourceHealthChecker::NewRelic(
            K8sHealthNRInstrumentation::new(
                k8s_client.clone(),
                type_meta,
                name.clone(),
                start_time,
            ),
        )]
    // No Health-checkers for any other type meta
    } else {
        trace!("No health-checkers for TypeMeta {type_meta:?}");
        vec![]
    }
}

/// This health-checker implementation contains a collection of [HealthChecker] that are queried to provide a
/// unified health value for agents in Kubernetes.
pub struct K8sHealthChecker<HC = K8sResourceHealthChecker>
where
    HC: HealthChecker,
{
    health_checkers: Vec<HC>,
    start_time: StartTime,
}

impl K8sHealthChecker<K8sResourceHealthChecker> {
    pub fn try_new(
        k8s_client: Arc<SyncK8sClient>,
        resources: Arc<Vec<DynamicObject>>,
        start_time: StartTime,
    ) -> Result<Option<Self>, HealthCheckerError> {
        let mut health_checkers = vec![];
        for resource in resources.iter() {
            let type_meta = resource.types.clone().ok_or(HealthCheckerError::Generic(
                "not able to build flux health checker: type not found".to_string(),
            ))?;

            let name = resource
                .metadata
                .clone()
                .name
                .ok_or(HealthCheckerError::Generic(
                    "not able to build flux health checker: name not found".to_string(),
                ))?;

            let resource_health_checkers =
                health_checkers_for_type_meta(type_meta, k8s_client.clone(), name, start_time);

            for health_checker in resource_health_checkers {
                health_checkers.push(health_checker);
            }
        }
        if health_checkers.is_empty() {
            return Ok(None);
        }
        Ok(Some(Self {
            health_checkers,
            start_time,
        }))
    }
}

impl<HC> HealthChecker for K8sHealthChecker<HC>
where
    HC: HealthChecker,
{
    fn check_health(&self) -> Result<HealthWithStartTime, HealthCheckerError> {
        for rhc in self.health_checkers.iter() {
            let health = rhc.check_health()?;
            if !health.is_healthy() {
                return Ok(health);
            }
        }
        Ok(HealthWithStartTime::from_healthy(
            Healthy::new(String::default()),
            self.start_time,
        ))
    }
}

#[cfg(test)]
pub mod tests {
    use crate::agent_control::config::{
        helmrelease_v2_type_meta, instrumentation_v1beta1_type_meta,
    };
    use crate::health::health_checker::tests::MockHealthCheck;
    use crate::health::health_checker::{HealthChecker, HealthCheckerError};
    use crate::health::k8s::health_checker::{K8sHealthChecker, K8sResourceHealthChecker};
    use crate::health::with_start_time::StartTime;
    use crate::k8s::client::MockSyncK8sClient;
    use assert_matches::assert_matches;
    use kube::api::{DynamicObject, TypeMeta};
    use std::sync::Arc;

    #[test]
    fn no_resource_set() {
        let mock_client = MockSyncK8sClient::default();
        assert!(
            K8sHealthChecker::try_new(Arc::new(mock_client), Arc::new(vec![]), StartTime::now())
                .unwrap()
                .is_none()
        )
    }
    #[test]
    fn failing_build_health_check_resource_with_no_type() {
        let mock_client = MockSyncK8sClient::default();

        assert_matches!(
            K8sHealthChecker::try_new(
                Arc::new(mock_client),
                Arc::new(vec![DynamicObject {
                    // having no type causes an error
                    types: None,
                    metadata: Default::default(),
                    data: Default::default(),
                }]),
                StartTime::now()
            )
            .err()
            .unwrap(),
            HealthCheckerError::Generic(s) => {
                assert_eq!(s, "not able to build flux health checker: type not found".to_string())
            }
        );
    }

    #[test]
    fn failing_build_health_check_resource_with_no_name() {
        let mock_client = MockSyncK8sClient::default();

        assert_matches!(
            K8sHealthChecker::try_new(
                Arc::new(mock_client),
                Arc::new(vec![DynamicObject {
                    types: Some(helmrelease_v2_type_meta()),
                    // having no name causes an error
                    metadata: Default::default(),
                    data: Default::default(),
                }]),
                StartTime::now()
            )
            .err()
            .unwrap(),
            HealthCheckerError::Generic(s) => {
                assert_eq!(s, "not able to build flux health checker: name not found".to_string())
            }
        );
    }

    #[test]
    fn successful_build_health_check_with_unsupported_type_meta() {
        let mock_client = MockSyncK8sClient::default();

        // Create a TypeMeta that is not supported by health_checkers_for_type_meta
        let unsupported_type_meta = TypeMeta {
            api_version: "unsupported/v1".to_string(),
            kind: "UnsupportedResource".to_string(),
        };

        let test_object = DynamicObject {
            types: Some(unsupported_type_meta),
            metadata: kube::core::ObjectMeta {
                name: Some("test-resource".to_string()),
                ..Default::default()
            },
            data: Default::default(),
        };

        let health_checker = K8sHealthChecker::try_new(
            Arc::new(mock_client),
            Arc::new(vec![test_object]),
            StartTime::now(),
        )
        .unwrap();

        assert!(health_checker.is_none());
    }

    #[test]
    fn successful_build_health_check_with_helmrelease_v2() {
        let mock_client = MockSyncK8sClient::default();
        let start_time = StartTime::now();

        let test_object = DynamicObject {
            types: Some(helmrelease_v2_type_meta()),
            metadata: kube::core::ObjectMeta {
                name: Some("test-helmrelease".to_string()),
                ..Default::default()
            },
            data: Default::default(),
        };

        let health_checker = K8sHealthChecker::try_new(
            Arc::new(mock_client),
            Arc::new(vec![test_object]),
            start_time,
        )
        .unwrap()
        .expect("the health-checker cannot be empty");

        assert_eq!(health_checker.health_checkers.len(), 4);
        assert_matches!(
            health_checker.health_checkers[0],
            K8sResourceHealthChecker::Flux(_)
        );
        assert_matches!(
            health_checker.health_checkers[1],
            K8sResourceHealthChecker::StatefulSet(_)
        );
        assert_matches!(
            health_checker.health_checkers[2],
            K8sResourceHealthChecker::DaemonSet(_)
        );
        assert_matches!(
            health_checker.health_checkers[3],
            K8sResourceHealthChecker::Deployment(_)
        );
    }

    #[test]
    fn successful_build_health_check_with_instrumentation_v1beta1() {
        let mock_client = MockSyncK8sClient::default();
        let start_time = StartTime::now();

        let test_object = DynamicObject {
            types: Some(instrumentation_v1beta1_type_meta()),
            metadata: kube::core::ObjectMeta {
                name: Some("test-instrumentation".to_string()),
                ..Default::default()
            },
            data: Default::default(),
        };

        let health_checker = K8sHealthChecker::try_new(
            Arc::new(mock_client),
            Arc::new(vec![test_object]),
            start_time,
        )
        .unwrap()
        .expect("The health-checkers cannot be empty");

        assert_eq!(health_checker.health_checkers.len(), 1);
        assert_matches!(
            health_checker.health_checkers[0],
            K8sResourceHealthChecker::NewRelic(_)
        );
    }

    #[test]
    fn logic_health_check() {
        let start_time = StartTime::now();
        assert!(
            K8sHealthChecker {
                health_checkers: vec![
                    MockHealthCheck::new_healthy(),
                    MockHealthCheck::new_healthy()
                ],
                start_time,
            }
            .check_health()
            .unwrap()
            .is_healthy()
        );

        assert!(
            !K8sHealthChecker {
                health_checkers: vec![
                    MockHealthCheck::new_healthy(),
                    MockHealthCheck::new_unhealthy(),
                    MockHealthCheck::new_healthy()
                ],
                start_time
            }
            .check_health()
            .unwrap()
            .is_healthy() //Notice that this assert has a ! at the beginning
        );

        assert!(
            K8sHealthChecker {
                health_checkers: vec![
                    MockHealthCheck::new_healthy(),
                    MockHealthCheck::new_with_error(),
                    MockHealthCheck::new_healthy()
                ],
                start_time
            }
            .check_health()
            .is_err()
        );
    }
}
