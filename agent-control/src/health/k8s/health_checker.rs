use crate::health::health_checker::{HealthChecker, HealthCheckerError, Healthy};
use crate::health::k8s::daemon_set::K8sHealthDaemonSet;
use crate::health::k8s::deployment::K8sHealthDeployment;
use crate::health::k8s::helm_release::K8sHealthFluxHelmRelease;
use crate::health::k8s::instrumentation::K8sHealthNRInstrumentation;
use crate::health::k8s::stateful_set::K8sHealthStatefulSet;
use crate::health::with_start_time::{HealthWithStartTime, StartTime};
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use kube::api::DynamicObject;
use resource_type::ResourceType;
use std::sync::Arc;
use tracing::trace;

mod resource_type;

// This label selector is added in post-render and present no matter the chart we are installing
// https://github.com/fluxcd/helm-controller/blob/main/CHANGELOG.md#090
pub const LABEL_RELEASE_FLUX: &str = "helm.toolkit.fluxcd.io/name";

/// This enum wraps all the health check implementations related to a Kubernetes resource.
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

            let Ok(resource_type) = (&type_meta).try_into() else {
                trace!("Unsupported resource type: {:?}. skipping.", type_meta);
                continue;
            };

            let name = resource
                .metadata
                .clone()
                .name
                .ok_or(HealthCheckerError::Generic(
                    "not able to build flux health checker: name not found".to_string(),
                ))?;

            match resource_type {
                ResourceType::HelmRelease => {
                    health_checkers.push(K8sResourceHealthChecker::Flux(
                        K8sHealthFluxHelmRelease::new(
                            k8s_client.clone(),
                            type_meta,
                            name.clone(),
                            start_time,
                        ),
                    ));

                    health_checkers.push(K8sResourceHealthChecker::StatefulSet(
                        K8sHealthStatefulSet::new(k8s_client.clone(), name.clone(), start_time),
                    ));

                    health_checkers.push(K8sResourceHealthChecker::DaemonSet(
                        K8sHealthDaemonSet::new(k8s_client.clone(), name.clone(), start_time),
                    ));

                    health_checkers.push(K8sResourceHealthChecker::Deployment(
                        K8sHealthDeployment::new(k8s_client.clone(), name, start_time),
                    ));
                }
                ResourceType::InstrumentationCRD => {
                    health_checkers.push(K8sResourceHealthChecker::NewRelic(
                        K8sHealthNRInstrumentation::new(
                            k8s_client.clone(),
                            type_meta,
                            name.clone(),
                            start_time,
                        ),
                    ));
                }
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
    use crate::agent_control::config::helmrelease_v2_type_meta;
    use crate::health::health_checker::tests::MockHealthCheck;
    use crate::health::health_checker::{HealthChecker, HealthCheckerError};
    use crate::health::k8s::health_checker::K8sHealthChecker;
    use crate::health::with_start_time::StartTime;
    use crate::k8s::client::MockSyncK8sClient;
    use assert_matches::assert_matches;
    use kube::api::DynamicObject;
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
