#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::sub_agent::health::health_checker::{
    Health, HealthChecker, HealthCheckerError, Healthy,
};
use crate::sub_agent::health::k8s::daemon_set::K8sHealthDaemonSet;
use crate::sub_agent::health::k8s::deployment::K8sHealthDeployment;
use crate::sub_agent::health::k8s::helm_release::K8sHealthFluxHelmRelease;
use crate::sub_agent::health::k8s::stateful_set::K8sHealthStatefulSet;
use crate::super_agent::config::helm_release_type_meta;
use kube::api::DynamicObject;
use std::sync::Arc;

// This label selector is added in post-render and present no matter the chart we are installing
// https://github.com/fluxcd/helm-controller/blob/main/CHANGELOG.md#090
pub const LABEL_RELEASE_FLUX: &str = "helm.toolkit.fluxcd.io/name";

/// K8sHealthChecker exists to wrap all the k8s health checks to have a unique array and a single loop
pub enum K8sHealthChecker {
    Flux(K8sHealthFluxHelmRelease),
    StatefulSet(K8sHealthStatefulSet),
    DaemonSet(K8sHealthDaemonSet),
    Deployment(K8sHealthDeployment),
}

impl HealthChecker for K8sHealthChecker {
    fn check_health(&self) -> Result<Health, HealthCheckerError> {
        match self {
            K8sHealthChecker::Flux(flux) => flux.check_health(),
            K8sHealthChecker::StatefulSet(stateful_set) => stateful_set.check_health(),
            K8sHealthChecker::DaemonSet(daemon_set) => daemon_set.check_health(),
            K8sHealthChecker::Deployment(deployment) => deployment.check_health(),
        }
    }
}

/// SubAgentHealthChecker contains a collection of healthChecks that are queried to provide a unified health value
pub struct SubAgentHealthChecker<HC = K8sHealthChecker>
where
    HC: HealthChecker,
{
    health_checkers: Vec<HC>,
}

impl SubAgentHealthChecker<K8sHealthChecker> {
    pub fn try_new(
        k8s_client: Arc<SyncK8sClient>,
        resources: Vec<DynamicObject>,
    ) -> Result<Self, HealthCheckerError> {
        let mut health_checkers = vec![];
        for resource in resources.iter() {
            let type_meta = resource.types.clone().ok_or(HealthCheckerError::Generic(
                "not able to build flux health checker: type not found".to_string(),
            ))?;
            if type_meta != helm_release_type_meta() {
                continue;
            }
            let name = resource
                .metadata
                .clone()
                .name
                .ok_or(HealthCheckerError::Generic(
                    "not able to build flux health checker: name not found".to_string(),
                ))?;

            health_checkers.push(K8sHealthChecker::Flux(K8sHealthFluxHelmRelease::new(
                k8s_client.clone(),
                name.clone(),
            )));

            health_checkers.push(K8sHealthChecker::StatefulSet(K8sHealthStatefulSet::new(
                k8s_client.clone(),
                name.clone(),
            )));

            health_checkers.push(K8sHealthChecker::DaemonSet(K8sHealthDaemonSet::new(
                k8s_client.clone(),
                name.clone(),
            )));

            health_checkers.push(K8sHealthChecker::Deployment(K8sHealthDeployment::new(
                k8s_client.clone(),
                name,
            )));
        }
        Ok(Self { health_checkers })
    }
}

impl<HC> HealthChecker for SubAgentHealthChecker<HC>
where
    HC: HealthChecker,
{
    fn check_health(&self) -> Result<Health, HealthCheckerError> {
        for rhc in self.health_checkers.iter() {
            let health = rhc.check_health()?;
            if !health.is_healthy() {
                return Ok(health);
            }
        }
        Ok(Healthy::default().into())
    }
}

#[cfg(test)]
pub mod test {
    use crate::k8s::client::MockSyncK8sClient;
    use crate::sub_agent::health::health_checker::test::MockHealthCheckMock;
    use crate::sub_agent::health::health_checker::{HealthChecker, HealthCheckerError};
    use crate::sub_agent::health::k8s::health_checker::SubAgentHealthChecker;
    use crate::super_agent::config::helm_release_type_meta;
    use assert_matches::assert_matches;
    use kube::api::DynamicObject;
    use std::sync::Arc;

    #[test]
    fn no_resource_set() {
        let mock_client = MockSyncK8sClient::default();
        assert!(
            SubAgentHealthChecker::try_new(Arc::new(mock_client), vec![])
                .unwrap()
                .check_health()
                .unwrap()
                .is_healthy()
        );
    }
    #[test]
    fn failing_build_health_check_resource_with_no_type() {
        let mock_client = MockSyncK8sClient::default();

        assert_matches!(
            SubAgentHealthChecker::try_new(
                Arc::new(mock_client),
                vec![DynamicObject {
                    // having no type causes an error
                    types: None,
                    metadata: Default::default(),
                    data: Default::default(),
                }]
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
            SubAgentHealthChecker::try_new(
                Arc::new(mock_client),
                vec![DynamicObject {
                    types: Some(helm_release_type_meta()),
                    // having no name causes an error
                    metadata: Default::default(),
                    data: Default::default(),
                }]
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
        assert!(SubAgentHealthChecker {
            health_checkers: vec![
                MockHealthCheckMock::new_healthy(),
                MockHealthCheckMock::new_healthy()
            ],
        }
        .check_health()
        .unwrap()
        .is_healthy());

        assert!(
            !SubAgentHealthChecker {
                health_checkers: vec![
                    MockHealthCheckMock::new_healthy(),
                    MockHealthCheckMock::new_unhealthy(),
                    MockHealthCheckMock::new_healthy()
                ],
            }
            .check_health()
            .unwrap()
            .is_healthy() //Notice that this assert has a ! at the beginning
        );

        assert!(SubAgentHealthChecker {
            health_checkers: vec![
                MockHealthCheckMock::new_healthy(),
                MockHealthCheckMock::new_with_error(),
                MockHealthCheckMock::new_healthy()
            ],
        }
        .check_health()
        .is_err());
    }
}
