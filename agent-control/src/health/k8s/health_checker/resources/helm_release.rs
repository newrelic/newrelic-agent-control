use crate::health::health_checker::{HealthChecker, HealthCheckerError, Healthy, Unhealthy};
use crate::health::with_start_time::{HealthWithStartTime, StartTime};
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use k8s_openapi::serde_json::{Map, Value};
use kube::api::TypeMeta;
use std::sync::Arc;

const CONDITION_READY: &str = "Ready";

/// Enumerates the possible statuses that a Kubernetes condition can report.
#[derive(Debug, PartialEq, Eq)]
enum ConditionStatus {
    True,
    False,
    Unknown,
}

impl From<&str> for ConditionStatus {
    fn from(s: &str) -> Self {
        match s {
            "True" => ConditionStatus::True,
            "False" => ConditionStatus::False,
            _ => ConditionStatus::Unknown,
        }
    }
}

/// Represents a health checker for a specific HelmRelease in Kubernetes.
///
/// This struct is designed to be used within a wrapper that manages multiple
/// instances, each corresponding to a different HelmRelease, allowing for
/// health checks across several Helm releases within a Kubernetes cluster.
pub struct K8sHealthFluxHelmRelease {
    k8s_client: Arc<SyncK8sClient>,
    type_meta: TypeMeta,
    name: String,
    start_time: StartTime,
}

impl K8sHealthFluxHelmRelease {
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

    /// Fetches and validates the 'status' field from the HelmRelease data.
    fn get_status(
        &self,
        helm_release_data: &Map<String, Value>,
    ) -> Result<Map<String, Value>, HealthCheckerError> {
        helm_release_data
            .get("status")
            .and_then(|s| s.as_object())
            .cloned()
            .ok_or_else(|| {
                HealthCheckerError::Generic(format!(
                    "Failed to parse status of HelmRelease '{}'",
                    &self.name
                ))
            })
    }

    /// Extracts the conditions from the status of the HelmRelease.
    fn get_status_conditions(
        &self,
        status: &Map<String, Value>,
    ) -> Result<Vec<Value>, HealthCheckerError> {
        let conditions = status
            .get("conditions")
            .and_then(|c| c.as_array())
            .cloned()
            .ok_or_else(|| {
                HealthCheckerError::Generic(format!(
                    "No conditions found in status of HelmRelease '{}'",
                    &self.name
                ))
            })?;
        Ok(conditions)
    }

    /// Finds the 'Ready' condition in a list of conditions.
    /// Iterates through conditions, returning the first 'Ready' condition found, if any.
    /// Returns `Some(condition)` if a 'Ready' condition is found, otherwise `None`.
    fn find_ready_condition(&self, conditions: &[Value]) -> Option<Value> {
        for cond in conditions {
            match cond.get("type").and_then(Value::as_str) {
                Some(cond_type) if cond_type == CONDITION_READY => return Some(cond.clone()),
                _ => continue,
            }
        }
        None
    }

    /// Evaluates the health of a HelmRelease based on the presence and status of its 'Ready' condition.
    /// Returns a tuple where the first element is a boolean indicating health,
    /// and the second is a message detailing the health status or issues found.
    fn is_healthy_and_message(&self, conditions: &[Value]) -> (bool, String) {
        let ready_condition = self.find_ready_condition(conditions);

        match ready_condition {
            Some(condition) => {
                match condition
                    .get("status")
                    .and_then(Value::as_str)
                    .map(ConditionStatus::from)
                {
                    Some(ConditionStatus::True) => (true, "HelmRelease is healthy".to_string()),
                    Some(ConditionStatus::False) => {
                        // If 'Ready' condition is false, return error with message if available
                        let message = condition
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("No specific message found");
                        (false, format!("HelmRelease not ready: {}", message))
                    }
                    _ => (false, "HelmRelease status unknown or missing".to_string()),
                }
            }
            None => (false, "No 'Ready' condition was found".to_string()),
        }
    }
}

impl HealthChecker for K8sHealthFluxHelmRelease {
    fn check_health(&self) -> Result<HealthWithStartTime, HealthCheckerError> {
        // Attempt to get the HelmRelease from Kubernetes
        let helm_release = self
            .k8s_client
            .get_dynamic_object(&self.type_meta, &self.name)
            .map_err(|e| {
                HealthCheckerError::Generic(format!(
                    "Error fetching HelmRelease '{}': {}",
                    &self.name, e
                ))
            })?
            .ok_or_else(|| {
                HealthCheckerError::Generic(format!("HelmRelease '{}' not found", &self.name))
            })?;

        let helm_release_data = helm_release.data.as_object().ok_or_else(|| {
            HealthCheckerError::Generic("HelmRelease data is not an object".to_string())
        })?;

        let status = self.get_status(helm_release_data)?;
        let conditions = self.get_status_conditions(&status)?;

        let (is_healthy, message) = self.is_healthy_and_message(&conditions);
        if is_healthy {
            Ok(HealthWithStartTime::from_healthy(
                Healthy::new(String::default()),
                self.start_time,
            ))
        } else {
            Ok(HealthWithStartTime::from_unhealthy(
                Unhealthy::new(String::default(), message),
                self.start_time,
            ))
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::agent_control::config::helmrelease_v2_type_meta;
    use crate::health::health_checker::Health;
    use crate::k8s::{Error, client::MockSyncK8sClient};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use kube::core::DynamicObject;
    use serde_json::json;

    #[test]
    fn test_helm_release() {
        type TestCase = (
            &'static str,
            Result<Health, HealthCheckerError>,
            fn(&mut MockSyncK8sClient),
        );
        let test_cases : Vec<TestCase> = vec![
            (
                "Helm release healthy when ready and status true",
                Ok(Healthy::default().into()),
                |mock: &mut MockSyncK8sClient| {
                    let status_conditions = json!({
                        "conditions": [
                            {"type": "Ready", "status": "True", "lastTransitionTime": "2021-01-01T12:00:00Z"},
                        ]
                    });
                    setup_mock_client_with_conditions(mock, status_conditions);
                },
            ),
            (
                "Helm release unhealthy when ready and status false",
                Ok(Unhealthy::new(String::default(),"HelmRelease not ready: test error".to_string()).into()),
                |mock: &mut MockSyncK8sClient| {
                    let status_conditions = json!({
                        "conditions": [
                            {"type": "Ready", "status": "False", "lastTransitionTime": "2021-01-01T12:00:00Z","message":"test error"},
                        ]
                    });
                    setup_mock_client_with_conditions(mock, status_conditions);
                },
            ),
            (
                "Helm release unhealthy when not ready conditions",
                Ok(Unhealthy::new(String::default(),"No 'Ready' condition was found".to_string()).into()),
                |mock: &mut MockSyncK8sClient| {
                    let status_conditions = json!({
                        "conditions": [
                            {"type": "Reconciling", "status": "True", "lastTransitionTime": "2021-01-02T12:00:00Z"}
                        ]
                    });
                    setup_mock_client_with_conditions(mock, status_conditions);
                },
            ),
            (
                "Helm release unhealthy when not ready and other true condition types",
                Ok(Unhealthy::new(String::default(),"HelmRelease not ready: No specific message found".to_string()).into()),
                |mock: &mut MockSyncK8sClient| {
                    let status_conditions = json!({
                        "conditions": [
                            {"type": "Ready", "status": "False", "lastTransitionTime": "2021-01-01T12:00:00Z"},
                            {"type": "Reconciling", "status": "True", "lastTransitionTime": "2021-01-02T12:00:00Z"}
                        ]
                    });
                    setup_mock_client_with_conditions(mock, status_conditions);
                },
            ),
            (
                "Helm release unhealthy when no conditions",
                Ok(Unhealthy::new(String::default(),"No 'Ready' condition was found".to_string()).into()),
                |mock: &mut MockSyncK8sClient| {
                    let status_conditions = json!({"conditions": []});
                    setup_mock_client_with_conditions(mock, status_conditions);
                },
            ),
            (
                "Error fetching HelmRelease",
                Err(HealthCheckerError::Generic(
                    "Error fetching HelmRelease 'example-release': while getting dynamic resource: Error".to_string(),
                )),
                |mock: &mut MockSyncK8sClient| {
                    mock.expect_get_dynamic_object()
                        .returning(|_,_| Err(Error::GetDynamic("Error".to_string())));
                },
            ),
        ];

        for (name, expected, setup_mock) in test_cases {
            let mut mock_client = MockSyncK8sClient::new();
            setup_mock(&mut mock_client);
            let start_time = StartTime::now();
            let checker = K8sHealthFluxHelmRelease::new(
                Arc::new(mock_client),
                helmrelease_v2_type_meta(),
                "example-release".to_string(),
                start_time,
            );
            let result = checker.check_health();
            match expected {
                Ok(expected_health) => {
                    let result_health =
                        result.unwrap_or_else(|err| panic!("Unexpected {err} - {name}"));
                    assert_eq!(
                        result_health,
                        HealthWithStartTime::new(expected_health, start_time)
                    );
                }
                Err(expected_err) => {
                    let result_err = result.unwrap_err();
                    assert_eq!(result_err.to_string(), expected_err.to_string(), "{}", name);
                }
            }
        }
    }

    fn setup_mock_client_with_conditions(
        mock: &mut MockSyncK8sClient,
        status_conditions: serde_json::Value,
    ) {
        mock.expect_get_dynamic_object()
            .withf(|_, name| name == "example-release")
            .times(1)
            .returning(move |_, _| {
                Ok(Some(Arc::new(DynamicObject {
                    types: Some(helmrelease_v2_type_meta()),
                    metadata: ObjectMeta::default(),
                    data: json!({
                        "status": status_conditions
                    }),
                })))
            });
    }
}
