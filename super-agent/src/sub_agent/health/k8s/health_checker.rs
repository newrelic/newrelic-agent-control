#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::sub_agent::health::health_checker::{
    Health, HealthChecker, HealthCheckerError, Healthy,
};
use crate::sub_agent::health::k8s::helm_release::K8sHealthFluxHelmRelease;
use crate::super_agent::config::helm_release_type_meta;
use kube::api::DynamicObject;
use std::sync::Arc;

// GenericHealthCheck exist to wrap all the k8s health checks to have a unique array and a single loop
pub enum GenericHealthCheck {
    Flux(K8sHealthFluxHelmRelease),
}

impl HealthChecker for GenericHealthCheck {
    fn check_health(&self) -> Result<Health, HealthCheckerError> {
        match self {
            GenericHealthCheck::Flux(flux) => flux.check_health(),
        }
    }
}

/// K8sHealthChecker contains a collection of healthChecks that are queried to provide a unified health value
pub struct K8sHealthChecker<HC = GenericHealthCheck>
where
    HC: HealthChecker,
{
    // Send is needed since K8sHealthChecker is passed to a different thread
    health_checkers: Vec<HC>,
}

impl K8sHealthChecker<GenericHealthCheck> {
    pub fn try_new(
        k8s_client: Arc<SyncK8sClient>,
        resources: Vec<DynamicObject>,
    ) -> Result<Self, HealthCheckerError> {
        let mut health_checkers = vec![];
        for resource in resources.iter() {
            let type_meta = resource.types.clone().ok_or(HealthCheckerError::new(
                "not able to build flux health checker: type not found".to_string(),
            ))?;
            if type_meta != helm_release_type_meta() {
                continue;
            }

            let name = resource
                .metadata
                .clone()
                .name
                .ok_or(HealthCheckerError::new(
                    "not able to build flux health checker: name not found".to_string(),
                ))?;

            health_checkers.push(GenericHealthCheck::Flux(K8sHealthFluxHelmRelease::new(
                k8s_client.clone(),
                name,
            )));
        }
        Ok(Self { health_checkers })
    }
}

impl<HC> HealthChecker for K8sHealthChecker<HC>
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
    use crate::sub_agent::health::health_checker::{Health, Healthy, Unhealthy};
    use crate::sub_agent::health::health_checker::{HealthChecker, HealthCheckerError};
    use crate::sub_agent::health::k8s::health_checker::K8sHealthChecker;
    use crate::super_agent::config::helm_release_type_meta;
    use kube::api::DynamicObject;
    use mockall::mock;
    use std::sync::Arc;

    #[test]
    fn no_resource_set() {
        let mock_client = MockSyncK8sClient::default();

        assert!(K8sHealthChecker::try_new(Arc::new(mock_client), vec![])
            .unwrap()
            .check_health()
            .unwrap()
            .is_healthy());
    }

    #[test]
    fn failing_build_health_check() {
        let mock_client = MockSyncK8sClient::default();

        assert_eq!(
            K8sHealthChecker::try_new(
                Arc::new(mock_client),
                vec![DynamicObject {
                    types: Some(helm_release_type_meta()),
                    metadata: Default::default(),
                    data: Default::default(),
                }]
            )
            .err()
            .unwrap(),
            HealthCheckerError::new(
                "not able to build flux health checker: name not found".to_string(),
            )
        );
    }

    mock! {
        pub HealthCheckMock{}
        impl HealthChecker for HealthCheckMock{
            fn check_health(&self) -> Result<Health, HealthCheckerError>;
        }
    }

    fn get_healthy_mock() -> MockHealthCheckMock {
        let mut healthy = MockHealthCheckMock::new();
        healthy
            .expect_check_health()
            .returning(|| Ok(Healthy::default().into()));
        healthy
    }

    fn get_unhealthy_mock() -> MockHealthCheckMock {
        let mut unhealthy = MockHealthCheckMock::new();
        unhealthy
            .expect_check_health()
            .returning(|| Ok(Unhealthy::default().into()));
        unhealthy
    }

    fn get_error_mock() -> MockHealthCheckMock {
        let mut unhealthy = MockHealthCheckMock::new();
        unhealthy
            .expect_check_health()
            .returning(|| Err(HealthCheckerError::new("test".to_string())));
        unhealthy
    }

    #[test]
    fn logic_health_check() {
        assert!(K8sHealthChecker {
            health_checkers: vec![get_healthy_mock(), get_healthy_mock()],
        }
        .check_health()
        .unwrap()
        .is_healthy());

        assert!(!K8sHealthChecker {
            health_checkers: vec![get_healthy_mock(), get_unhealthy_mock(), get_healthy_mock()],
        }
        .check_health()
        .unwrap()
        .is_healthy());

        assert!(K8sHealthChecker {
            health_checkers: vec![get_healthy_mock(), get_error_mock(), get_healthy_mock()],
        }
        .check_health()
        .is_err());
    }
}
