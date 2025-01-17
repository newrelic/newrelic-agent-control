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
/// The `Instrumentation` CR structure which contains this `status` field in K8s also contains a
/// field `Instrumentation.status.lastUpdated`, this represents when the statuses were written
/// because a status changed.
/// It does not represent when the health was last checked or anything from a health response, so we
/// do not use it here
#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct InstrumentationStatus {
    /// [`pods_matching`] is the number of pods which match the
    /// `Instrumentation.spec.podLabelSelectors` and `Instrumentation.spec.NamespaceLabelSelectors`
    /// (when empty defaults to matching everything, and both must match to "select" a pod).
    #[serde(default)]
    pods_matching: u64,

    /// [`pods_healthy`] is the number of pods which match based on [`pods_matching`] and
    /// [`pods_injected`] and the operator was able to get:
    /// - The correct pod IP/port.
    /// - A health response which had a healthy status reported via the yaml field healthy.
    /// - An http status code of 200.
    #[serde(default)]
    pods_healthy: u64,

    /// [`pods_injected`] is the number of pods which matched the Instrumentation based on
    /// [`pods_matching`] which had the the health sidecar injected.
    #[serde(default)]
    pods_injected: u64,

    /// [`pods_not_ready`] is the number of pods which are not in a ready state
    /// (`Pod.status.phase` != `"Running"`) which matched both [`pods_matching`]
    /// and [`pods_injected`].
    #[serde(default)]
    pods_not_ready: u64,

    /// [`pods_outdated`] is the number of pods which match based on [`pods_matching`] and
    /// [`pods_injected`] but there's a mismatch between the `Instrumentation.generation` and
    /// the injected pods annotation (to identify changes to the spec).
    #[serde(default)]
    pods_outdated: u64,

    /// [`pods_unhealthy`] is the number of pods which failed a health check, either because
    /// the operator couldn't get the pod ip, pod port, communication issues, timeout,
    /// non-200 http status, failure to decode the http response, and lastly
    /// the `last_error` field in the response.
    #[serde(default)]
    pods_unhealthy: u64,

    /// [`unhealthy_pod_errors`] is a list of pods (namespace.name/pod.name) and either
    /// the last error from the response
    /// or the error from the operator while trying to collect health.
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
    /// `"podsMatching:1, podsHealthy:1, podsInjected:1, podsNotReady:0, podsOutdated:0, podsUnhealthy:0"`
    /// It returns an Healthy value if:
    /// `not_ready` <= 0
    /// `healthy` > 0
    /// `unhealthy` <= 0
    /// `matching` > 0
    /// `matching` == `injected`
    /// We can't rely on the number of healthy pods the same as matching pods because there can be
    /// uninstrumented or outdated pods so the matching will be higher. We just consider healthy
    /// any case not being one of the previous cases.
    pub(crate) fn get_health(&self) -> Health {
        if self.is_healthy() {
            Health::Healthy(Healthy::new(self.to_string()))
        } else {
            Health::Unhealthy(Unhealthy::new(self.to_string(), self.last_error()))
        }
    }

    // If this changes please align the docs here: <https://newrelic.atlassian.net/wiki/spaces/INST/pages/3945988387/K8s+Retrieving+health+from+Instrumentation+CR+s+status#Agent-Control-logic>
    fn is_healthy(&self) -> bool {
        // All pods must be ready
        self.pods_not_ready == 0
        // No unhealthy pods
        && self.pods_unhealthy == 0
        // At least one pod healthy
        && self.pods_healthy > 0
        // There should be matching pods, else the instrumentation is not doing anything
        && self.pods_matching > 0
        // The pods that match should have been injected
        && self.pods_injected == self.pods_matching
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
    fn default_instrumentation_value_evals_to_unhealthy() {
        let status = InstrumentationStatus::default();

        assert!(matches!(status.get_health(), Health::Unhealthy(_)));
    }

    #[test]
    fn json_failing_serde() {
        let status_jsons = [
            serde_json::json!(1),
            serde_json::json!(true),
            serde_json::json!("podsMatching"),
            serde_json::json!(["podsMatching"]),
            serde_json::json!([{"podsMatching": 1}]),
            serde_json::json!(null),
        ];

        status_jsons.into_iter().for_each(|status_json| {
            assert!(serde_json::from_value::<InstrumentationStatus>(status_json).is_err())
        });
    }

    #[test]
    fn json_empty_collections_can_be_deserialized() {
        let status_jsons = [serde_json::json!([]), serde_json::json!({})];

        status_jsons.into_iter().for_each(|status_json| {
            assert!(serde_json::from_value::<InstrumentationStatus>(status_json).is_ok())
        });
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
            TestData {
                case: "missing fields",
                json: serde_json::json!({
                    "podsMatching": 1,
                    "podsHealthy": 1,
                    "podsInjected": 1,
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

        data_table.into_iter().for_each(|data| {
            assert_eq!(
                serde_json::from_value::<InstrumentationStatus>(data.json.clone()).unwrap(),
                data.expected,
                "failed case '{}'",
                data.case
            );
        });
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
                expected: Health::Unhealthy(Unhealthy::new(
                    "podsMatching:0, podsHealthy:0, podsInjected:0, podsNotReady:0, podsOutdated:0, podsUnhealthy:0"
                        .to_string(), String::default()
                )),
            },
            TestData {
                case: "healthy case",
                status: InstrumentationStatus {
                    pods_matching: 1,
                    pods_healthy: 1,
                    pods_injected: 1,
                    ..Default::default()
                },
                expected: Health::Healthy(Healthy::new(
                    "podsMatching:1, podsHealthy:1, podsInjected:1, podsNotReady:0, podsOutdated:0, podsUnhealthy:0"
                        .to_string()
                )),
            },
            TestData {
                case: "unhealthy case",
                status: InstrumentationStatus {
                    pods_matching: 1,
                    pods_healthy: 1,
                    ..Default::default()
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
                    pods_unhealthy: 1,
                    unhealthy_pods_errors: vec![UnhealthyPodError {
                        pod: "pod1".to_string(),
                        last_error: "error1".to_string(),
                    }],
                    ..Default::default()
                },
                expected: Health::Unhealthy(Unhealthy::new(
                    "podsMatching:1, podsHealthy:1, podsInjected:1, podsNotReady:0, podsOutdated:0, podsUnhealthy:1"
                        .to_string(),
                    "pod pod1:error1".to_string(),
                )),},
                TestData {
                    case: "unhealthy case with multiple errors",
                    status: InstrumentationStatus {
                        pods_matching: 1,
                        pods_healthy: 1,
                        pods_injected: 1,
                        pods_unhealthy: 2,
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
                        ..Default::default()
                    },
                    expected: Health::Unhealthy(Unhealthy::new(
                        "podsMatching:1, podsHealthy:1, podsInjected:1, podsNotReady:0, podsOutdated:0, podsUnhealthy:2"
                            .to_string(),
                        "pod pod1:error1, pod pod2:error2".to_string(),
                    )),
                },
                TestData {
                    case: "0 pods matching",
                    status: InstrumentationStatus {
                        pods_matching: 0,
                        pods_healthy: 1,
                        pods_injected: 1,
                        pods_not_ready: 1,
                        pods_outdated: 1,
                        pods_unhealthy: 1,
                        ..Default::default()
                    },
                    expected: Health::Unhealthy(Unhealthy::new(
                        "podsMatching:0, podsHealthy:1, podsInjected:1, podsNotReady:1, podsOutdated:1, podsUnhealthy:1"
                            .to_string(),
                        "".to_string(),
                    )),

                },
                TestData {
                    case: "0 healthy pods",
                    status: InstrumentationStatus {
                        pods_matching: 1,
                        pods_healthy: 0,
                        pods_injected: 1,
                        pods_not_ready: 1,
                        pods_outdated: 1,
                        pods_unhealthy: 1,
                        ..Default::default()
                    },
                    expected: Health::Unhealthy(Unhealthy::new(
                        "podsMatching:1, podsHealthy:0, podsInjected:1, podsNotReady:1, podsOutdated:1, podsUnhealthy:1"
                            .to_string(),
                        "".to_string(),
                    )),
                },

                TestData {
                    case: "0 injected pods",
                    status: InstrumentationStatus {
                        pods_matching: 1,
                        pods_healthy: 1,
                        pods_injected: 0,
                        pods_not_ready: 1,
                        pods_outdated: 1,
                        pods_unhealthy: 1,
                        ..Default::default()
                    },
                    expected: Health::Unhealthy(Unhealthy::new(
                        "podsMatching:1, podsHealthy:1, podsInjected:0, podsNotReady:1, podsOutdated:1, podsUnhealthy:1"
                            .to_string(),
                        "".to_string(),
                    )),
                },

                TestData {
                    case: "0 not ready pods but unhealthy",
                    status: InstrumentationStatus {
                        pods_matching: 1,
                        pods_healthy: 1,
                        pods_injected: 1,
                        pods_not_ready: 0,
                        pods_outdated: 1,
                        pods_unhealthy: 1,
                        ..Default::default()
                    },
                    expected: Health::Unhealthy(Unhealthy::new(
                        "podsMatching:1, podsHealthy:1, podsInjected:1, podsNotReady:0, podsOutdated:1, podsUnhealthy:1"
                            .to_string(),
                        "".to_string(),
                    )),
                },

                TestData {
                    case: "matching != injected",
                    status: InstrumentationStatus {
                        pods_matching: 1,
                        pods_healthy: 1,
                        pods_injected: 2,
                        pods_not_ready: 1,
                        pods_outdated: 1,
                        pods_unhealthy: 1,
                        ..Default::default()
                    },
                    expected: Health::Unhealthy(Unhealthy::new(
                        "podsMatching:1, podsHealthy:1, podsInjected:2, podsNotReady:1, podsOutdated:1, podsUnhealthy:1"
                            .to_string(),
                        "".to_string(),
                    )),
                },
                TestData {
                    case: "not ready pods",
                    status: InstrumentationStatus {
                        pods_matching: 1,
                        pods_healthy: 1,
                        pods_injected: 1,
                        pods_not_ready: 1,
                        pods_outdated: 1,
                        pods_unhealthy: 1,
                        ..Default::default()
                    },
                    expected: Health::Unhealthy(Unhealthy::new(
                        "podsMatching:1, podsHealthy:1, podsInjected:1, podsNotReady:1, podsOutdated:1, podsUnhealthy:1"
                            .to_string(),
                        "".to_string(),
                    )),
                },


        ];

        data_table.into_iter().for_each(|data| {
            assert_eq!(
                data.status.get_health(),
                data.expected,
                "failed case '{}'",
                data.case
            );
        });
    }
}
