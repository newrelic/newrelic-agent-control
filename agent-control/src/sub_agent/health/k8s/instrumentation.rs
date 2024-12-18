#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::sub_agent::health::health_checker::{
    Health, HealthChecker, HealthCheckerError, Healthy, Unhealthy,
};
use crate::sub_agent::health::with_start_time::{HealthWithStartTime, StartTime};
use kube::api::TypeMeta;
use serde::Deserialize;
use std::fmt::Display;
use std::sync::Arc;

/// Represents the status of an Instrumentation CRD in K8s, as of apiVersion: newrelic.com/v1alpha2.
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
    type_meta: TypeMeta,
    name: String,
    start_time: StartTime,
}

impl K8sHealthNRInstrumentation {
    pub fn new(
        k8s_client: Arc<SyncK8sClient>,
        type_meta: TypeMeta,
        name: String,
        start_time: StartTime,
    ) -> Self {
        Self {
            k8s_client,
            type_meta,
            name,
            start_time,
        }
    }
}

impl HealthChecker for K8sHealthNRInstrumentation {
    fn check_health(&self) -> Result<HealthWithStartTime, HealthCheckerError> {
        // Attempt to get the Instrumentation from Kubernetes
        let instrumentation = self
            .k8s_client
            .get_dynamic_object(&self.type_meta, &self.name)
            .map_err(|e| {
                HealthCheckerError::Generic(format!(
                    "instrumentation CR could not be fetched'{}': {}",
                    &self.name, e
                ))
            })?
            .ok_or_else(|| {
                HealthCheckerError::Generic(format!("Instrumentation '{}' not found", &self.name))
            })?;

        let instrumentation_data = instrumentation.data.as_object().ok_or_else(|| {
            HealthCheckerError::Generic("instrumentation CR data is not an object".to_string())
        })?;

        let status = instrumentation_data.get("status").cloned().ok_or_else(|| {
            HealthCheckerError::Generic("instrumentation status could not be retrieved".to_string())
        })?;

        let status: InstrumentationStatus = serde_json::from_value(status).map_err(|e| {
            HealthCheckerError::Generic(format!(
                "could not deserialize a valid instrumentation status: {}",
                e
            ))
        })?;

        Ok(HealthWithStartTime::new(
            status.get_health(),
            self.start_time,
        ))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    #[test]
    fn get_healthiness_basic() {
        let status = InstrumentationStatus::default();

        assert!(matches!(status.get_health(), Health::Healthy(_)));
    }

    #[test]
    fn json_failing_serde() {
        let status_jsons = [
            serde_json::json!({}),
            serde_json::json!([]),
            serde_json::json!(null),
            serde_json::json!(1),
            serde_json::json!(true),
        ];

        for status_json in status_jsons.iter() {
            let status: Result<InstrumentationStatus, _> =
                serde_json::from_value(status_json.clone());
            assert!(status.is_err());
        }
    }

    #[test]
    fn json_serde() {
        struct TestData {
            case: &'static str,
            json: Value,
            expected: InstrumentationStatus,
        }

        let data_table = [
            TestData {
                case: "basic",
                json: serde_json::json!({
                    "podsMatching": 1,
                    "podsHealthy": 1,
                    "podsInjected": 1,
                    "podsNotReady": 0,
                    "podsOutdated": 0,
                    "podsUnhealthy": 0,
                }),
                expected: InstrumentationStatus {
                    pods_matching: 1,
                    pods_healthy: 1,
                    pods_injected: 1,
                    pods_not_ready: 0,
                    pods_outdated: 0,
                    pods_unhealthy: 0,
                    unhealthy_pods_errors: vec![],
                },
            },
            TestData {
                case: "with errors",
                json: serde_json::json!({
                    "podsMatching": 1,
                    "podsHealthy": 1,
                    "podsInjected": 1,
                    "podsNotReady": 0,
                    "podsOutdated": 0,
                    "podsUnhealthy": 1,
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
                }),
                expected: InstrumentationStatus {
                    pods_matching: 1,
                    pods_healthy: 1,
                    pods_injected: 1,
                    pods_not_ready: 0,
                    pods_outdated: 0,
                    pods_unhealthy: 1,
                    unhealthy_pods_errors: vec![
                        UnhealthyPodError {
                            pod: "pod1".to_string(),
                            last_error: "error1".to_string(),
                        },
                        UnhealthyPodError {
                            pod: "pod2".to_string(),
                            last_error: "error2".to_string(),
                        },
                    ],
                },
            },
        ];

        for data in data_table.iter() {
            let status: InstrumentationStatus = serde_json::from_value(data.json.clone()).unwrap();
            assert_eq!(status, data.expected, "failed case '{}'", data.case);
        }
    }

    #[test]
    fn status_health_checks() {
        struct TestData {
            case: &'static str,
            status: InstrumentationStatus,
            expected: Health,
        }
        let data_table = [
            TestData {
                case: "default case",
                status: InstrumentationStatus::default(),
                expected: Health::Healthy(Healthy::new(
                    "podsMatching:0, podsHealthy:0, podsInjected:0, podsNotReady:0, podsOutdated:0, podsUnhealthy:0"
                        .to_string(),
                )),
            },
            TestData {
                case: "healthy case",
                status: InstrumentationStatus {
                    pods_matching: 1,
                    pods_healthy: 1,
                    pods_injected: 1,
                    pods_not_ready: 1,
                    pods_outdated: 0,
                    pods_unhealthy: 0,
                    unhealthy_pods_errors: vec![],
                },
                expected: Health::Unhealthy(Unhealthy::new(
                    "podsMatching:1, podsHealthy:1, podsInjected:1, podsNotReady:1, podsOutdated:0, podsUnhealthy:0"
                        .to_string(),
                    "".to_string(),
                )),
            },
            TestData {
                case: "unhealthy case",
                status: InstrumentationStatus {
                    pods_matching: 1,
                    pods_healthy: 1,
                    pods_injected: 0,
                    pods_not_ready: 0,
                    pods_outdated: 0,
                    pods_unhealthy: 0,
                    unhealthy_pods_errors: vec![],
                },
                expected: Health::Unhealthy(Unhealthy::new(
                    "podsMatching:1, podsHealthy:1, podsInjected:0, podsNotReady:0, podsOutdated:0, podsUnhealthy:0"
                        .to_string(),
                    "".to_string(),
                )),
            },
            TestData {
                case: "unhealthy case with errors",
                status: InstrumentationStatus {
                    pods_matching: 1,
                    pods_healthy: 1,
                    pods_injected: 1,
                    pods_not_ready: 0,
                    pods_outdated: 0,
                    pods_unhealthy: 1,
                    unhealthy_pods_errors: vec![UnhealthyPodError {
                        pod: "pod1".to_string(),
                        last_error: "error1".to_string(),
                    }],
                },
                expected: Health::Unhealthy(Unhealthy::new(
                    "podsMatching:1, podsHealthy:1, podsInjected:1, podsNotReady:0, podsOutdated:0, podsUnhealthy:1"
                        .to_string(),
                    "pod pod1:error1".to_string(),
                )),},
        ];

        for data in data_table.iter() {
            assert_eq!(
                data.status.get_health(),
                data.expected,
                "failed case '{}'",
                data.case
            );
        }
    }
}
