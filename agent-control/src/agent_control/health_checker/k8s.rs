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
    ac_release_name: Option<String>,
    cd_release_name: Option<String>,
) -> impl Fn(SystemTime) -> Option<K8sHealthChecker> {
    move |start_time: SystemTime| {
        // ac_release_name existing means AC is enabled
        let mut ac_checkers = ac_release_name
            .as_ref()
            .map(|release_name| {
                health_checkers_for_type_meta(
                    helmrelease_v2_type_meta(),
                    k8s_client.clone(),
                    release_name.clone(),
                    namespace.clone(),
                    Some(namespace.clone()),
                    start_time,
                )
            })
            .unwrap_or_default();

        // cd_release_name existing means flux is enabled
        let cd_checkers = cd_release_name
            .as_ref()
            .map(|release_name| {
                health_checkers_for_type_meta(
                    helmrelease_v2_type_meta(),
                    k8s_client.clone(),
                    release_name.clone(),
                    namespace.clone(),
                    Some(namespace.clone()),
                    start_time,
                )
            })
            .unwrap_or_default();

        ac_checkers.extend(cd_checkers);

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
        let agent_control_release_name = "ac-deployment".to_string();
        let cd_release_name = "flux-cd".to_string();

        let builder_fn = agent_control_health_checker_builder(
            mock_k8s_client,
            namespace,
            Some(agent_control_release_name),
            Some(cd_release_name),
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
        let agent_control_release_name = "ac-deployment".to_string();

        let builder_fn = agent_control_health_checker_builder(
            mock_k8s_client,
            namespace,
            Some(agent_control_release_name),
            None,
        );
        let health_checker = builder_fn(SystemTime::now()).expect("Builder should not return None");

        assert_eq!(
            health_checker.checkers_count(),
            4,
            "There should only be 1 checker when Flux is disabled"
        );
    }
}
