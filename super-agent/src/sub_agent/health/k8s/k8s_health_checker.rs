#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::sub_agent::health::health_checker::{
    Health, HealthChecker, HealthCheckerError, Healthy,
};
use crate::sub_agent::health::k8s::helm_release::K8sHealthFluxHelmRelease;
use crate::super_agent::config::helm_release_type_meta;
use kube::api::DynamicObject;
use std::sync::Arc;
use std::time::Duration;

/// K8sHealthChecker contains a collection of healthChecks that are queried to provide a unified helath value
pub struct K8sHealthChecker {
    releases_helm_checkers: Vec<K8sHealthFluxHelmRelease>,
    interval: Duration,
}

impl K8sHealthChecker {
    pub fn try_new(
        k8s_client: Arc<SyncK8sClient>,
        resources: Vec<DynamicObject>,
        interval: Duration,
    ) -> Result<Self, HealthCheckerError> {
        Ok(Self {
            releases_helm_checkers: K8sHealthChecker::get_releases_helm_checkers(
                k8s_client.clone(),
                resources,
            )?,
            interval,
        })
    }

    fn get_releases_helm_checkers(
        k8s_client: Arc<SyncK8sClient>,
        resources: Vec<DynamicObject>,
    ) -> Result<Vec<K8sHealthFluxHelmRelease>, HealthCheckerError> {
        let mut flux_releases_helm_checkers = vec![];
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

            flux_releases_helm_checkers
                .push(K8sHealthFluxHelmRelease::new(k8s_client.clone(), name));
        }
        Ok(flux_releases_helm_checkers)
    }
}

impl HealthChecker for K8sHealthChecker {
    fn check_health(&self) -> Result<Health, HealthCheckerError> {
        for rhc in self.releases_helm_checkers.iter() {
            let health = rhc.check_health()?;
            if !health.is_healthy() {
                return Ok(health);
            }
        }

        Ok(Health::Healthy(Healthy::default()))
    }

    fn interval(&self) -> Duration {
        self.interval
    }
}

#[cfg(test)]
pub mod test {
    use crate::k8s::client::MockSyncK8sClient;
    use crate::sub_agent::health::health_checker::HealthChecker;
    use crate::sub_agent::health::k8s::helm_release::test::setup_mock_client_with_conditions;
    use crate::sub_agent::health::k8s::k8s_health_checker::K8sHealthChecker;
    use crate::super_agent::config::helm_release_type_meta;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use kube::api::DynamicObject;
    use serde_json::json;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn no_resource_set() {
        let mock_client = MockSyncK8sClient::default();

        assert!(
            K8sHealthChecker::try_new(Arc::new(mock_client), vec![], Duration::from_secs(3))
                .unwrap()
                .check_health()
                .unwrap()
                .is_healthy()
        );
    }

    #[test]
    fn failing_build_health_check() {
        let mock_client = MockSyncK8sClient::default();

        assert!(K8sHealthChecker::try_new(
            Arc::new(mock_client),
            vec![DynamicObject {
                types: Some(helm_release_type_meta()),
                metadata: Default::default(),
                data: Default::default(),
            }],
            Duration::from_secs(3)
        )
        .is_err());
    }

    #[test]
    fn failing_health_check() {
        let mut mock_client = MockSyncK8sClient::default();
        let status_conditions = json!({
            "conditions": [
                {"type": "Ready", "status": "False", "lastTransitionTime": "2021-01-01T12:00:00Z"},
            ]
        });
        setup_mock_client_with_conditions(&mut mock_client, status_conditions);

        let monitored_obj = DynamicObject {
            types: Some(helm_release_type_meta()),
            data: Default::default(),
            metadata: ObjectMeta {
                name: Some("example-release".to_string()),
                ..Default::default()
            },
        };

        assert!(!K8sHealthChecker::try_new(
            Arc::new(mock_client),
            vec![monitored_obj],
            Duration::from_secs(3)
        )
        .unwrap()
        .check_health()
        .unwrap()
        .is_healthy())
    }
}
