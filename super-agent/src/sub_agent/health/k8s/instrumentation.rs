#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::sub_agent::health::health_checker::{
    Health, HealthChecker, HealthCheckerError, Healthy, Unhealthy,
};
use crate::sub_agent::health::with_start_time::{HealthWithStartTime, StartTime};
use k8s_openapi::serde_json::{Map, Value};
use kube::api::DynamicObject;
use std::collections::HashMap;
use std::sync::Arc;

// CONSTANTS THAT ARE KEYS TO NUMERIC VALUES
const NUM_PODS_MATCHING: &str = "podsMatching";
const NUM_PODS_HEALTHY: &str = "podsHealthy";
const NUM_PODS_INJECTED: &str = "podsInjected";
const NUM_PODS_NOT_READY: &str = "podsNotReady";
const NUM_PODS_OUTDATED: &str = "podsOutdated";
const NUM_PODS_UNHEALTHY: &str = "podsUnhealthy";
const STATUS_ENTRIES: [&str; 6] = [
    NUM_PODS_MATCHING,
    NUM_PODS_HEALTHY,
    NUM_PODS_INJECTED,
    NUM_PODS_NOT_READY,
    NUM_PODS_OUTDATED,
    NUM_PODS_UNHEALTHY,
];

/// Key for the last errors of the unhealthy pods in the status of an Instrumentation
const UNHEALTHY_PODS_ERRORS: &str = "unhealthyPodsErrors";
const LAST_ERROR_LABEL: &str = "lastError";
const POD_LABEL: &str = "pod";

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

    /// Evaluates the healthiness from an Instrumentation, it returns a status with the following:
    /// "podsMatching:1, podsHealthy:1, podsInjected:1, podsNotReady:0, podsOutdated:0, podsUnhealthy:0"
    /// It returns a Healthy or Unhealthy type depending on the conditions:
    /// not_ready > 0 --> Unhealthy
    /// Matching != Injected --> Unhealthy
    /// Unhealthy > 0 ---> Unhealthy with lastErrors
    /// We can't rely on the number of healthy pods lower than matching because there can be uninstrumented
    /// or outdated pods so the matching will be higher, so we just consider healthy
    /// any case not being one of the previous cases.
    fn get_healthiness(status: &Map<String, Value>) -> Health {
        let status_entries: HashMap<_, _> = STATUS_ENTRIES
            .into_iter()
            .map(|status_entry| {
                let entry_val = status
                    .get(status_entry)
                    .and_then(Value::as_i64)
                    .unwrap_or_default();
                (status_entry, entry_val)
            })
            .collect();

        let status_msg = Self::get_status_msg(&status_entries);

        let pods_matching = status_entries
            .get(NUM_PODS_MATCHING)
            .copied()
            .unwrap_or_default();

        if pods_matching <= 0 {
            return Health::Healthy(Healthy::new(status_msg));
        }

        let pods_not_ready = status_entries
            .get(NUM_PODS_NOT_READY)
            .copied()
            .unwrap_or_default();

        let pods_injected = status_entries
            .get(NUM_PODS_INJECTED)
            .copied()
            .unwrap_or_default();

        let pods_unhealthy = status_entries
            .get(NUM_PODS_UNHEALTHY)
            .copied()
            .unwrap_or_default();

        let is_unhealthy =
            pods_not_ready > 0 || pods_injected != pods_matching || pods_unhealthy > 0;

        if is_unhealthy {
            // Get last errors of the unhealthy pods, if any
            // Should we log if the array length does not match the number reported by `podsUnhealthy`?
            let last_errors = status
                .get(UNHEALTHY_PODS_ERRORS)
                .and_then(Value::as_array)
                .map(|arr| Self::get_last_errors(arr))
                .unwrap_or_default();

            Health::Unhealthy(Unhealthy::new(status_msg, last_errors))
        } else {
            Health::Healthy(Healthy::new(status_msg))
        }
    }

    /// Iterates over the status entries of an Instrumentation and returns a comma-separated string
    /// with the status of the Instrumentation.
    fn get_status_msg(status_entries: &HashMap<&str, i64>) -> String {
        let status_msg = status_entries
            .iter()
            .map(|(status_entry, entry_val)| format!("{}:{}", status_entry, entry_val));
        comma_separated_msg(status_msg)
    }

    /// Iterates over the `unhealthyPodsErrors` array in the status of an Instrumentation and
    /// returns a comma-separated string with the last errors of the unhealthy pods, if any.
    ///
    /// This does not check if the length of the `unhealthyPodsErrors` array is the same as the
    /// number reported in the `podsUnhealthy` field. We work under the assumption that
    /// the information is consistent.
    fn get_last_errors(unhealthy_pods_errors: &[Value]) -> String {
        let last_errors = unhealthy_pods_errors.iter().map(|unhealthy| {
            let pod_id = unhealthy
                .get(POD_LABEL)
                .and_then(Value::as_str)
                .unwrap_or_default();
            let last_error = unhealthy
                .get(LAST_ERROR_LABEL)
                .and_then(Value::as_str)
                .unwrap_or_default();
            format!("pod {}:{}", pod_id, last_error)
        });

        comma_separated_msg(last_errors)
    }
}

/// Creates a comma-separated string from an iterator of strings
fn comma_separated_msg(msg_arr: impl Iterator<Item = String>) -> String {
    msg_arr.collect::<Vec<_>>().join(", ")
}

impl HealthChecker for K8sHealthNRInstrumentation {
    fn check_health(&self) -> Result<HealthWithStartTime, HealthCheckerError> {
        // Attempt to get the Instrumentation from Kubernetes
        let instrumentation = self
            .k8s_client
            .get_instrumentation(&self.name)
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

        Ok(HealthWithStartTime::new(
            Self::get_healthiness(instrumentation_data),
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
