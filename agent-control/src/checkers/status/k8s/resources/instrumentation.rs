use crate::checkers::status::{AgentStatus, StatusCheckError, StatusChecker};
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use kube::api::TypeMeta;
use serde::Deserialize;
use std::sync::Arc;
use tracing::warn;

#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstrumentationStatus {
    #[serde(default)]
    pub entity_guids: Vec<String>,
}

#[derive(Debug)]
pub struct K8sStatusInstrumentation {
    k8s_client: Arc<SyncK8sClient>,
    type_meta: TypeMeta,
    name: String,
    namespace: String,
}

impl K8sStatusInstrumentation {
    pub fn new(
        k8s_client: Arc<SyncK8sClient>,
        type_meta: TypeMeta,
        name: String,
        namespace: String,
    ) -> Self {
        Self {
            k8s_client,
            type_meta,
            name,
            namespace,
        }
    }

    fn validate_guids(&self, guids: &[String]) -> Result<String, StatusCheckError> {
        if guids.is_empty() {
            return Err(StatusCheckError(
                "Instrumentation status has empty 'entityGuids'".to_string(),
            ));
        }

        let first_guid = &guids[0];
        let all_match = guids.iter().all(|g| g == first_guid);

        if !all_match {
            return Err(StatusCheckError(format!(
                "Mismatching entity GUIDs found in status: {:?}",
                guids
            )));
        }

        Ok(first_guid.clone())
    }
}

impl StatusChecker for K8sStatusInstrumentation {
    fn check_status(&self) -> Result<AgentStatus, StatusCheckError> {
        let obj_opt = self
            .k8s_client
            .get_dynamic_object(&self.type_meta, &self.name, &self.namespace)
            .map_err(|e| StatusCheckError(format!("K8s error: {e}")))?;

        let Some(obj) = obj_opt else {
            return Err(StatusCheckError(format!(
                "Instrumentation {}/{} not found",
                self.namespace, self.name
            )));
        };

        let status_value = obj.data.get("status").ok_or_else(|| {
            StatusCheckError("Instrumentation resource has no 'status' field".to_string())
        })?;

        let instr_status: InstrumentationStatus = serde_json::from_value(status_value.clone())
            .map_err(|e| {
                warn!("Failed to deserialize InstrumentationStatus: {}", e);
                StatusCheckError(format!("Invalid status structure: {e}"))
            })?;

        let valid_guid = self.validate_guids(&instr_status.entity_guids)?;

        Ok(AgentStatus {
            status: valid_guid,
            opamp_field: "guid".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg_attr(test, mockall_double::double)]
    use crate::k8s::client::SyncK8sClient;

    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use kube::api::DynamicObject;
    use mockall::predicate::*;
    use serde_json::json;
    use std::sync::Arc;

    fn setup_mock_client(
        status_payload: Option<serde_json::Value>,
        k8s_error: Option<String>,
    ) -> SyncK8sClient {
        let mut client = SyncK8sClient::new();

        let tm = TypeMeta {
            api_version: "v1".into(),
            kind: "Instrumentation".into(),
        };

        let name = "test-inst";
        let namespace = "default";

        if let Some(err_msg) = k8s_error {
            client
                .expect_get_dynamic_object()
                .with(eq(tm), eq(name), eq(namespace))
                .returning(move |_, _, _| {
                    Err(crate::k8s::error::K8sError::Generic(err_msg.clone()))
                });
        } else if let Some(json_val) = status_payload {
            let obj = DynamicObject {
                types: Some(tm.clone()),
                metadata: ObjectMeta {
                    name: Some(name.into()),
                    namespace: Some(namespace.into()),
                    ..Default::default()
                },
                data: json_val,
            };

            client
                .expect_get_dynamic_object()
                .with(eq(tm), eq(name), eq(namespace))
                .returning(move |_, _, _| Ok(Some(Arc::new(obj.clone()))));
        } else {
            client
                .expect_get_dynamic_object()
                .with(eq(tm), eq(name), eq(namespace))
                .returning(|_, _, _| Ok(None));
        }

        client
    }

    fn get_checker(client: SyncK8sClient) -> K8sStatusInstrumentation {
        K8sStatusInstrumentation::new(
            Arc::new(client),
            TypeMeta {
                api_version: "v1".into(),
                kind: "Instrumentation".into(),
            },
            "test-inst".into(),
            "default".into(),
        )
    }

    #[test]
    fn test_success_single_guid() {
        let payload = json!({
            "status": {
                "entityGuids": ["GUID-123"]
            }
        });

        let client = setup_mock_client(Some(payload), None);
        let checker = get_checker(client);

        let result = checker.check_status().unwrap();
        assert_eq!(result.status, "GUID-123");
    }

    #[test]
    fn test_success_multiple_identical_guids() {
        let payload = json!({
            "status": {
                "entityGuids": ["GUID-ABC", "GUID-ABC", "GUID-ABC"]
            }
        });

        let client = setup_mock_client(Some(payload), None);
        let checker = get_checker(client);

        let result = checker.check_status().unwrap();
        assert_eq!(result.status, "GUID-ABC");
    }

    #[test]
    fn test_fail_empty_guids() {
        let payload = json!({
            "status": {
                "entityGuids": []
            }
        });

        let client = setup_mock_client(Some(payload), None);
        let checker = get_checker(client);

        let err = checker.check_status().unwrap_err();
        assert!(err.0.contains("empty"));
    }

    #[test]
    fn test_fail_mismatch_guids() {
        let payload = json!({
            "status": {
                "entityGuids": ["GUID-A", "GUID-B"]
            }
        });

        let client = setup_mock_client(Some(payload), None);
        let checker = get_checker(client);

        let err = checker.check_status().unwrap_err();
        assert!(err.0.contains("Mismatching"));
    }

    #[test]
    fn test_fail_missing_status_field() {
        let payload = json!({
            "spec": { "some": "config" }
        });

        let client = setup_mock_client(Some(payload), None);
        let checker = get_checker(client);

        let err = checker.check_status().unwrap_err();
        assert!(err.0.contains("no 'status' field"));
    }

    #[test]
    fn test_fail_missing_entity_guids_field() {
        let payload = json!({
            "status": {
                "phase": "Running"
            }
        });

        let client = setup_mock_client(Some(payload), None);
        let checker = get_checker(client);

        let err = checker.check_status().unwrap_err();
        assert!(err.0.contains("empty"));
    }

    #[test]
    fn test_fail_resource_not_found() {
        let client = setup_mock_client(None, None);
        let checker = get_checker(client);

        let err = checker.check_status().unwrap_err();
        assert!(err.0.contains("not found"));
    }

    #[test]
    fn test_fail_k8s_client_error() {
        let client = setup_mock_client(None, Some("Connection refused".to_string()));
        let checker = get_checker(client);

        let err = checker.check_status().unwrap_err();
        assert!(err.0.contains("K8s error"));
        assert!(err.0.contains("Connection refused"));
    }
}
