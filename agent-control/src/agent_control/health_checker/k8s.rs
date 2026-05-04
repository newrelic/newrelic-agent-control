use crate::agent_control::config::{K8sConfig, helmrelease_v2_type_meta};
use crate::checkers::health::health_checker::{HealthChecker, HealthCheckerError};
use crate::checkers::health::k8s::health_checker::{
    K8sHealthChecker, K8sResourceHealthChecker, health_checkers_for_type_meta,
};
use crate::checkers::health::noop::NoOpHealthChecker;
use crate::checkers::health::with_start_time::HealthWithStartTime;
use crate::k8s::client::K8sClient;
use std::{sync::Arc, time::SystemTime};

pub enum HealthCheckerVariants<C: K8sClient> {
    K8s(K8sHealthChecker<K8sResourceHealthChecker<C>>),
    NoOp(NoOpHealthChecker),
}

impl<C: K8sClient> HealthChecker for HealthCheckerVariants<C> {
    fn check_health(&self) -> Result<HealthWithStartTime, HealthCheckerError> {
        match self {
            Self::K8s(hc) => hc.check_health(),
            Self::NoOp(hc) => hc.check_health(),
        }
    }
}

/// Returns the builder function for the health-checker of Agent Control in Kubernetes.
pub fn agent_control_health_checker_builder<C: K8sClient>(
    k8s_client: Arc<C>,
    k8s_config: &K8sConfig,
) -> impl Fn(SystemTime) -> Option<HealthCheckerVariants<C>> {
    move |start_time: SystemTime| {
        // If CD is not enabled, we can skip all the checks related to it and return
        // a NoOpHealthChecker and we return always healthy if AC is up and running
        if !k8s_config.cd_enabled {
            return Some(HealthCheckerVariants::NoOp(NoOpHealthChecker::new(
                start_time,
            )));
        }

        let releases = [
            // ac_release_name existing means AC is enabled
            k8s_config.ac_release_name.as_ref(),
            // cd_release_name existing means flux is enabled
            k8s_config.cd_release_name.as_ref(),
        ]
        .into_iter()
        .flatten();
        let checkers: Vec<K8sResourceHealthChecker<C>> = releases
            .flat_map(|release_name| {
                health_checkers_for_type_meta(
                    helmrelease_v2_type_meta(),
                    k8s_client.clone(),
                    release_name.clone(),
                    k8s_config.namespace.clone(),
                    Some(k8s_config.namespace.clone()),
                    start_time,
                )
            })
            .collect();

        Some(HealthCheckerVariants::K8s(K8sHealthChecker::new(
            checkers, start_time,
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::k8s::client::tests::MockK8sClient;
    use std::sync::Arc;
    use std::time::SystemTime;

    #[test]
    fn test_builder_includes_flux_when_enabled() {
        let mock_k8s_client = Arc::new(MockK8sClient::new());
        let namespace = "test-ns".to_string();
        let agent_control_release_name = "ac-deployment".to_string();
        let cd_release_name = "flux-cd".to_string();

        let k8s_config = K8sConfig {
            namespace: namespace.clone(),
            ac_release_name: Some(agent_control_release_name.clone()),
            cd_release_name: Some(cd_release_name.clone()),
            cd_enabled: true,
            ..Default::default()
        };

        let builder_fn = agent_control_health_checker_builder(mock_k8s_client, &k8s_config);
        let health_checker = builder_fn(SystemTime::now()).expect("Builder should not return None");

        match health_checker {
            HealthCheckerVariants::K8s(hc) => {
                // Assuming each release (AC and Flux) contributes 4 checkers, we expect 8 total
                assert_eq!(
                    hc.checkers_count(),
                    8,
                    "There should be 8 checkers (4 for AC, 4 for Flux) when Flux is enabled"
                );
            }
            _ => panic!("Expected K8sHealthChecker variant"),
        }
    }

    #[test]
    fn test_builder_excludes_flux_when_disabled() {
        let mock_k8s_client = Arc::new(MockK8sClient::new());
        let namespace = "test-ns".to_string();
        let agent_control_release_name = "ac-deployment".to_string();

        let k8s_config = K8sConfig {
            namespace: namespace.clone(),
            ac_release_name: Some(agent_control_release_name.clone()),
            cd_enabled: true,
            ..Default::default()
        };

        let builder_fn = agent_control_health_checker_builder(mock_k8s_client, &k8s_config);
        let health_checker = builder_fn(SystemTime::now()).expect("Builder should not return None");

        match health_checker {
            HealthCheckerVariants::K8s(hc) => {
                // Assuming each release (AC and Flux) contributes 4 checkers, we expect 8 total
                assert_eq!(
                    hc.checkers_count(),
                    4,
                    "There should only be 4 checkers (all for AC) when Flux is disabled"
                );
            }
            _ => panic!("Expected K8sHealthChecker variant"),
        }
    }

    #[test]
    fn test_builder_returns_noop_when_cd_disabled() {
        let mock_k8s_client = Arc::new(MockK8sClient::new());

        let k8s_config = K8sConfig {
            namespace: "test-ns".to_string(),
            ac_release_name: Some("ac-deployment".to_string()),
            cd_enabled: false,
            ..Default::default()
        };

        let builder_fn = agent_control_health_checker_builder(mock_k8s_client, &k8s_config);
        let health_checker = builder_fn(SystemTime::now()).expect("Builder should not return None");

        assert!(
            matches!(health_checker, HealthCheckerVariants::NoOp(_)),
            "Expected NoOpHealthChecker variant when cd_enabled is false"
        );
    }
}
