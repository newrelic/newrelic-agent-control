use std::{sync::Arc, time::SystemTime};

#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::{
    agent_control::config::helmrelease_v2_type_meta,
    health::k8s::health_checker::{K8sHealthChecker, health_checkers_for_type_meta},
};

/// Returns the builder function for the health-checker of Agent Control in Kubernetes.
pub fn agent_control_health_checker_builder(
    k8s_client: Arc<SyncK8sClient>,
    namespace: String,
    ac_release_name: String,
) -> impl Fn(SystemTime) -> Option<K8sHealthChecker> {
    move |start_time: SystemTime| {
        let mut ac_checkers = health_checkers_for_type_meta(
            helmrelease_v2_type_meta(),
            k8s_client.clone(),
            AGENT_CONTROL_DEPLOYMENT_RELEASE_NAME.to_string(),
            namespace.clone(),
            Some(namespace.clone()),
            start_time,
        );

        if flux_enabled {
            let cd_checkers = health_checkers_for_type_meta(
                helmrelease_v2_type_meta(),
                k8s_client.clone(),
                ac_release_name.clone(),
                namespace.clone(),
                Some(namespace.clone()),
                start_time,
            );
            ac_checkers.extend(cd_checkers);
        }

        Some(K8sHealthChecker::new(ac_checkers, start_time))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::k8s::client::MockSyncK8sClient;
    use std::sync::Arc;
    use std::time::SystemTime;

    #[test]
    fn test_builder_includes_flux_when_enabled() {
        let mock_k8s_client = Arc::new(MockSyncK8sClient::new());
        let namespace = "test-ns".to_string();
        let cd_release_name = "flux-cd".to_string();
        let flux_enabled = true;

        let builder_fn = agent_control_health_checker_builder(
            mock_k8s_client,
            namespace,
            cd_release_name,
            flux_enabled,
        );

        let health_checker = builder_fn(SystemTime::now()).expect("Builder should not return None");

        assert_eq!(
            health_checker.checkers_count(),
            8,
            "There should be 2 checkers when Flux is enabled"
        );
    }

    #[test]
    fn test_builder_excludes_flux_when_disabled() {
        let mock_k8s_client = Arc::new(MockSyncK8sClient::new());
        let namespace = "test-ns".to_string();
        let cd_release_name = "flux-cd".to_string();
        let flux_enabled = false;

        let builder_fn = agent_control_health_checker_builder(
            mock_k8s_client,
            namespace,
            cd_release_name,
            flux_enabled,
        );
        let health_checker = builder_fn(SystemTime::now()).expect("Builder should not return None");

        assert_eq!(
            health_checker.checkers_count(),
            4,
            "There should only be 1 checker when Flux is disabled"
        );
    }
}
