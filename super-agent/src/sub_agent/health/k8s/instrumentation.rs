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

/// Key for an array of unhealthy pods in the status of an Instrumentation
const UNHEALTHY_PODS: &str = "unhealthyPods";

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

        let status_msg = status_entries
            .iter()
            .map(|(status_entry, entry_val)| format!("{}:{}", status_entry, entry_val))
            .collect::<Vec<_>>()
            .join(", ");

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
            let last_errors = Self::get_last_errors(status);
            Health::Unhealthy(Unhealthy::new(status_msg, last_errors))
        } else {
            Health::Healthy(Healthy::new(status_msg))
        }
    }
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
