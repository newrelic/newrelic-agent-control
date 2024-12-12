#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::sub_agent::health::health_checker::{
    Health, HealthChecker, HealthCheckerError, Healthy, Unhealthy,
};
use crate::sub_agent::health::with_start_time::{HealthWithStartTime, StartTime};
use crate::super_agent::config::helm_release_type_meta;
use kube::api::DynamicObject;
use serde::Deserialize;
use std::fmt::Display;
use std::sync::Arc;

/// Represents the status of an Instrumentation CRD in Kubernetes.
///
/// To be deserialized correctly, the JSON should have the following fields:
/// - `podsMatching` (int): The number of pods that match the Instrumentation.
/// - `podsHealthy` (int): The number of healthy pods.
/// - `podsInjected` (int): The number of pods that have been injected.
/// - `podsNotReady` (int): The number of pods that are not ready.
/// - `podsOutdated` (int): The number of outdated pods.
/// - `podsUnhealthy` (int): The number of unhealthy pods.
///
/// The following fields are optional:
/// - `unhealthyPodsErrors` (array): An array of objects with the following fields:
///   - `pod` (string): The name of the pod.
///   - `lastError` (string): The last error message.
#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct InstrumentationStatus {
    pods_matching: i64,
    pods_healthy: i64,
    pods_injected: i64,
    pods_not_ready: i64,
    pods_outdated: i64,
    pods_unhealthy: i64,
    #[serde(default)]
    unhealthy_pods_errors: Vec<UnhealthyPodError>,
}

impl Display for InstrumentationStatus {
    /// Formats the status as a string with the following format:
    /// "podsMatching:1, podsHealthy:1, podsInjected:1, podsNotReady:0, podsOutdated:0, podsUnhealthy:0"
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "podsMatching:{}, podsHealthy:{}, podsInjected:{}, podsNotReady:{}, podsOutdated:{}, podsUnhealthy:{}",
            self.pods_matching,
            self.pods_healthy,
            self.pods_injected,
            self.pods_not_ready,
            self.pods_outdated,
            self.pods_unhealthy,
        )
    }
}

impl InstrumentationStatus {
    /// Evaluates the healthiness from an Instrumentation, it returns a status with the following:
    /// "podsMatching:1, podsHealthy:1, podsInjected:1, podsNotReady:0, podsOutdated:0, podsUnhealthy:0"
    /// It returns a Healthy or Unhealthy type depending on the conditions:
    /// not_ready > 0 --> Unhealthy
    /// Matching != Injected --> Unhealthy
    /// Unhealthy > 0 ---> Unhealthy with lastErrors
    /// We can't rely on the number of healthy pods lower than matching because there can be uninstrumented
    /// or outdated pods so the matching will be higher, so we just consider healthy
    /// any case not being one of the previous cases.
    pub(crate) fn get_health(&self) -> Health {
        if self.pods_matching <= 0 || self.is_healthy() {
            Health::Healthy(Healthy::new(self.to_string()))
        } else {
            // Should we log if the array length does not match the number reported by `podsUnhealthy`?
            Health::Unhealthy(Unhealthy::new(self.to_string(), self.last_error()))
        }
    }

    fn is_healthy(&self) -> bool {
        self.pods_not_ready <= 0
            && self.pods_injected == self.pods_matching
            && self.pods_unhealthy <= 0
    }

    fn last_error(&self) -> String {
        self.unhealthy_pods_errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Represents the last errors of the unhealthy pods in the status of an Instrumentation CRD.
#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct UnhealthyPodError {
    pod: String,
    last_error: String,
}

impl Display for UnhealthyPodError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "pod {}:{}", self.pod, self.last_error)
    }
}

/// Represents a health checker for a specific Instrumentation in Kubernetes.
///
/// This struct is designed to be used within a wrapper that manages multiple
/// instances, each corresponding to a different instrumentation, allowing for
/// health checks across several instrumentations within a Kubernetes cluster.
pub struct K8sHealthNRInstrumentation {
    k8s_client: Arc<SyncK8sClient>,
    name: String,
    k8s_object: DynamicObject,
    start_time: StartTime,
}

impl K8sHealthNRInstrumentation {
    pub fn new(
        k8s_client: Arc<SyncK8sClient>,
        name: String,
        k8s_object: DynamicObject,
        start_time: StartTime,
    ) -> Self {
        Self {
            k8s_client,
            name,
            k8s_object,
            start_time,
        }
    }
}

impl HealthChecker for K8sHealthNRInstrumentation {
    fn check_health(&self) -> Result<HealthWithStartTime, HealthCheckerError> {
        // Attempt to get the Instrumentation from Kubernetes
        let instrumentation = self
            .k8s_client
            .get_dynamic_object(&helm_release_type_meta(), &self.name)
            .map_err(|e| {
                HealthCheckerError::Generic(format!(
                    "Error fetching Instrumentation '{}': {}",
                    &self.name, e
                ))
            })?
            .ok_or_else(|| {
                HealthCheckerError::Generic(format!("Instrumentation '{}' not found", &self.name))
            })?;

        let instrumentation_data = instrumentation.data.as_object().ok_or_else(|| {
            HealthCheckerError::Generic("Instrumentation data is not an object".to_string())
        })?;

        // Check if the instrumentation is properly updated: it should reflect the agent's configuration
        if self
            .k8s_client
            .has_dynamic_object_changed(&self.k8s_object)?
        {
            return Ok(HealthWithStartTime::from_unhealthy(
                Unhealthy::new(
                    String::default(),
                    format!(
                        "Instrumentation '{}' does not match the latest agent configuration",
                        &self.name,
                    ),
                ),
                self.start_time,
            ));
        }

        let status = instrumentation_data.get("status").cloned().ok_or_else(|| {
            HealthCheckerError::Generic("Instrumentation status could not be retrieved".to_string())
        })?;

        let status: InstrumentationStatus = serde_json::from_value(status).map_err(|e| {
            HealthCheckerError::Generic(format!("Error deserializing status: {}", e))
        })?;

        Ok(HealthWithStartTime::new(
            status.get_health(),
            self.start_time,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::sub_agent::health::k8s::instrumentation::comma_separated_msg;

    #[test]
    fn comma_separated() {
        let msg_arr = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(comma_separated_msg(msg_arr.into_iter()), "a, b, c");
    }

    #[test]
    fn comma_separated_one_item() {
        let msg_arr = vec!["a".to_string()];
        assert_eq!(comma_separated_msg(msg_arr.into_iter()), "a");
    }

    #[test]
    fn comma_separated_empty() {
        let msg_arr = Vec::<String>::default();
        assert_eq!(comma_separated_msg(msg_arr.into_iter()), "");
    }

    #[test]
    fn comma_separated_empty_string() {
        let msg_arr = vec!["".to_string()];
        assert_eq!(comma_separated_msg(msg_arr.into_iter()), "");
    }

    #[test]
    fn comma_separated_empty_strings() {
        let msg_arr = vec!["".to_string(), "".to_string(), "".to_string()];
        assert_eq!(comma_separated_msg(msg_arr.into_iter()), ", , ");
    }

    #[test]
    fn get_healthiness_basic() {
        let status = Map::default();

        assert!(matches!(
            K8sHealthNRInstrumentation::get_healthiness(&status),
            Health::Healthy(_)
        ));
    }

    #[test]
    fn get_healthiness_basic_healthy() {
        let status_json = serde_json::json!({
            "podsMatching": 1,
            "podsHealthy": 1,
            "podsInjected": 1,
            "podsNotReady": 0,
            "podsOutdated": 0,
            "podsUnhealthy": 0,
        });

        let status = status_json.as_object().unwrap();
        assert!(matches!(
            K8sHealthNRInstrumentation::get_healthiness(status),
            Health::Healthy(_)
        ));
    }

    #[test]
    fn get_healthiness_status_msg() {
        let status_json = serde_json::json!({
            "podsMatching": 1,
            "podsHealthy": 1,
            "podsInjected": 1,
            "podsNotReady": 0,
            "podsOutdated": 0,
            "podsUnhealthy": 0,
        });

        let status = status_json.as_object().unwrap();
        let health = K8sHealthNRInstrumentation::get_healthiness(status);
        let status = health.status();
        // Ordering might differ.
        // TODO do we want to sort?
        assert!(status.contains("podsMatching:1"));
        assert!(status.contains("podsHealthy:1"));
        assert!(status.contains("podsInjected:1"));
        assert!(status.contains("podsNotReady:0"));
        assert!(status.contains("podsOutdated:0"));
        assert!(status.contains("podsUnhealthy:0"));
    }

    // not_ready > 0 --> Unhealthy
    #[test]
    fn get_healthiness_not_ready() {
        let status_json = serde_json::json!({
            "podsMatching": 1,
            "podsHealthy": 1,
            "podsInjected": 1,
            "podsNotReady": 1,
            "podsOutdated": 0,
            "podsUnhealthy": 0,
        });

        let status = status_json.as_object().unwrap();
        assert!(matches!(
            K8sHealthNRInstrumentation::get_healthiness(status),
            Health::Unhealthy(_)
        ));
    }

    // Matching != Injected --> Unhealthy
    #[test]
    fn get_healthiness_injected() {
        let status_json = serde_json::json!({
            "podsMatching": 1,
            "podsHealthy": 1,
            "podsInjected": 0,
            "podsNotReady": 0,
            "podsOutdated": 0,
            "podsUnhealthy": 0,
        });

        let status = status_json.as_object().unwrap();
        assert!(matches!(
            K8sHealthNRInstrumentation::get_healthiness(status),
            Health::Unhealthy(_)
        ));
    }

    // Unhealthy > 0 ---> Unhealthy with lastErrors
    #[test]
    fn get_healthiness_unhealthy() {
        let status_json = serde_json::json!({
            "podsMatching": 1,
            "podsHealthy": 1,
            "podsInjected": 1,
            "podsNotReady": 0,
            "podsOutdated": 0,
            "podsUnhealthy": 1,
        });

        let status = status_json.as_object().unwrap();
        assert!(matches!(
            K8sHealthNRInstrumentation::get_healthiness(status),
            Health::Unhealthy(_)
        ));
    }

    // Unhealthy > 0 ---> Unhealthy with lastErrors
    #[test]
    fn get_healthiness_unhealthy_with_errors() {
        let status_json = serde_json::json!({
            "podsMatching": 1,
            "podsHealthy": 1,
            "podsInjected": 1,
            "podsNotReady": 0,
            "podsOutdated": 0,
            "podsUnhealthy": 1, // Note this number is different from the number of errors below!!
            "unhealthyPodsErrors": [
                {
                    "pod": "pod1",
                    "lastError": "error1"
                },
                {
                    "pod": "pod2",
                    "lastError": "error2"
                }
            ]
        });

        let status = status_json.as_object().unwrap();
        let health = K8sHealthNRInstrumentation::get_healthiness(status);
        let last_error = health.last_error().unwrap();

        assert!(matches!(health, Health::Unhealthy(_)));

        // Ordering might differ.
        // TODO do we want to sort?
        assert!(last_error.contains("pod pod1:error1"));
        assert!(last_error.contains("pod pod2:error2"));
    }
}
