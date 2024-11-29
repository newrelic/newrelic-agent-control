#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::sub_agent::health::health_checker::{
    Health, HealthChecker, HealthCheckerError, Healthy, Unhealthy,
};
use crate::sub_agent::health::k8s::helm_release::K8sHealthFluxHelmRelease;
use crate::sub_agent::health::with_start_time::{HealthWithStartTime, StartTime};
use k8s_openapi::merge_strategies::list::map;
use k8s_openapi::serde_json::{Map, Value};
use kube::api::DynamicObject;
use std::collections::HashMap;
use std::sync::Arc;
use semver::Op;

const PODS_MATCHING: &str = "podsMatching";
const PODS_HEALTHY: &str = "podsHealthy";
const PODS_INJECTED: &str = "podsInjected";
const PODS_NOT_READY: &str = "podsNotReady";
const PODS_OUTDATED: &str = "podsOutdated";
const PODS_UNHEALTHY: &str = "podsUnhealthy";

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
    /// Unhealthy > 0 ---> Unhealthy whit lastErrors
    /// We can't rely on the number of healthy pods lower than matching because there can be uninstrumented
    /// or outdated pods so the matching will be higher, so we just consider healthy
    /// any case not being one of the previous cases.
    fn get_healthiness(&self, status: Map<String, Value>) -> Health {
        let mut status_msg: String = String::default();
        let mut status_entries: HashMap<&str, i64> = HashMap::default();
        for status_entry in [
            PODS_MATCHING,
            PODS_HEALTHY,
            PODS_INJECTED,
            PODS_NOT_READY,
            PODS_OUTDATED,
            PODS_UNHEALTHY,
        ] {
            let entry_val = status
                .get(status_entry)
                .and_then(|ph| ph.as_i64())
                .unwrap_or(0);
            status_entries.insert(status_entry, entry_val);
            if !status_msg.is_empty() {
                status_msg.push_str(", ");
            }
            status_msg.push_str(format!("{}:{}", status_entry, entry_val).as_str());
        }

        let health = status_entries
            .get(PODS_MATCHING)
            .map(|pods_matching| {
                let matching = *pods_matching;
                if matching > 0 {
                    let mut is_healthy = true;
                    let mut last_errors = String::default();

                    let not_ready = status_entries.get(PODS_NOT_READY).unwrap_or(&0);
                    if *not_ready > 0 {
                        is_healthy = false;
                    }

                    let injected = status_entries.get(PODS_INJECTED).unwrap_or(&0);
                    if *injected != matching {
                        is_healthy = false;
                    }

                    let unhealthy = status_entries.get(PODS_HEALTHY).unwrap_or(&0);
                    if *unhealthy > 0 {
                        is_healthy = false;
                        let unhealthy_pods = status
                            .get("unhealthyPods")
                            .and_then(|up| up.as_array().cloned())
                            .unwrap_or_default();
                        for unhealthy in unhealthy_pods {
                            let pod_id = unhealthy
                                .get("pod")
                                .and_then(|up| up.as_str())
                                .unwrap_or("");
                            let last_error = unhealthy
                                .get("lastError")
                                .and_then(|up| up.as_str())
                                .unwrap_or("");
                            if !last_errors.is_empty() {
                                last_errors.push_str(", ");
                            }
                            last_errors.push_str(format!("pod {}:{}", pod_id, last_error).as_str());
                        }
                    }

                    if !is_healthy {
                        return Health::Unhealthy(Unhealthy::new(status_msg, last_errors));
                    }
                }
                Health::Healthy(Healthy::new(status_msg))
            })
            .expect("podsMatching should be defined at least with default 0");

        health
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
            self.get_healthiness(instrumentation_data.clone()),
            self.start_time,
        ))
    }
}
