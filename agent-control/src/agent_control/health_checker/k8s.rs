use std::{sync::Arc, time::SystemTime};

use crate::cli::install::agent_control::RELEASE_NAME;
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
) -> impl Fn(SystemTime) -> Option<K8sHealthChecker> {
    move |start_time: SystemTime| {
        Some(K8sHealthChecker::new(
            health_checkers_for_type_meta(
                helmrelease_v2_type_meta(),
                k8s_client.clone(),
                RELEASE_NAME.to_string(),
                namespace.clone(),
                Some(namespace.clone()),
                start_time,
            ),
            start_time,
        ))
    }
}
