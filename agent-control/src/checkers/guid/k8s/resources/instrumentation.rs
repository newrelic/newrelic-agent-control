use crate::checkers::guid::{EntityGuid, GuidCheckError, GuidChecker};
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use kube::api::TypeMeta;
use serde::Deserialize;
use std::sync::Arc;
use tracing::warn;

#[derive(Debug, Default, Deserialize, PartialEq)]
pub struct InstrumentationGuid {
    #[serde(default, rename = "entityGUIDs")]
    pub entity_guids: Vec<String>,
}

#[derive(Debug)]
pub struct K8sGuidInstrumentation {
    k8s_client: Arc<SyncK8sClient>,
    type_meta: TypeMeta,
    name: String,
    namespace: String,
    /// The field of the OpAMP payload where the retrieved version will be stored.
    ///
    /// Currently, this is always an identifying_attribute.
    opamp_field: String,
}

impl K8sGuidInstrumentation {
    pub fn new(
        k8s_client: Arc<SyncK8sClient>,
        type_meta: TypeMeta,
        name: String,
        namespace: String,
        opamp_field: String,
    ) -> Self {
        Self {
            k8s_client,
            type_meta,
            name,
            namespace,
            opamp_field,
        }
    }

    fn validate_guids(&self, guids: &[String]) -> Result<String, GuidCheckError> {
        if guids.is_empty() {
            return Err(GuidCheckError(
                "Instrumentation guid has empty 'entityGUIDs'".to_string(),
            ));
        }

        let first_guid = &guids[0];
        let all_match = guids.iter().all(|g| g == first_guid);

        if !all_match {
            return Err(GuidCheckError(format!(
                "Mismatching entity GUIDs found in status: {:?}",
                guids
            )));
        }

        Ok(first_guid.clone())
    }
}

impl GuidChecker for K8sGuidInstrumentation {
    fn check_guid(&self) -> Result<EntityGuid, GuidCheckError> {
        let obj_opt = self
            .k8s_client
            .get_dynamic_object(&self.type_meta, &self.name, &self.namespace)
            .map_err(|e| GuidCheckError(format!("K8s error: {e}")))?;

        let obj = obj_opt.ok_or_else(|| {
            GuidCheckError(format!(
                "Instrumentation {}/{} not found",
                self.namespace, self.name
            ))
        })?;

        let status_value = obj.data.get("status").ok_or_else(|| {
            GuidCheckError("Instrumentation resource has no 'status' field".to_string())
        })?;

        let instr_status: InstrumentationGuid = serde_json::from_value(status_value.clone())
            .map_err(|e| {
                warn!("Failed to deserialize InstrumentationStatus: {}", e);
                GuidCheckError(format!("Invalid status structure: {e}"))
            })?;

        let valid_guid = self.validate_guids(&instr_status.entity_guids)?;

        Ok(EntityGuid {
            guid: valid_guid,
            opamp_field: self.opamp_field.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg_attr(test, mockall_double::double)]
    use crate::k8s::client::SyncK8sClient;

    use crate::agent_control::defaults::APM_APPLICATION_ID;
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

    fn get_checker(client: SyncK8sClient) -> K8sGuidInstrumentation {
        K8sGuidInstrumentation::new(
            Arc::new(client),
            TypeMeta {
                api_version: "v1".into(),
                kind: "Instrumentation".into(),
            },
            "test-inst".into(),
            "default".into(),
            APM_APPLICATION_ID.to_string(),
        )
    }

    #[test]
    fn test_success_single_guid() {
        let payload = json!({
            "status": {
                "entityGUIDs": ["GUID-123"]
            }
        });

        let client = setup_mock_client(Some(payload), None);
        let checker = get_checker(client);

        let result = checker.check_guid().unwrap();
        assert_eq!(result.guid, "GUID-123");
    }

    #[test]
    fn test_success_multiple_identical_guids() {
        let payload = json!({
            "status": {
                "entityGUIDs": ["GUID-ABC", "GUID-ABC", "GUID-ABC"]
            }
        });

        let client = setup_mock_client(Some(payload), None);
        let checker = get_checker(client);

        let result = checker.check_guid().unwrap();
        assert_eq!(result.guid, "GUID-ABC");
    }

    #[test]
    fn test_fail_empty_guids() {
        let payload = json!({
            "status": {
                "entityGUIDs": []
            }
        });

        let client = setup_mock_client(Some(payload), None);
        let checker = get_checker(client);

        let err = checker.check_guid().unwrap_err();
        assert!(err.0.contains("empty"));
    }

    #[test]
    fn test_fail_mismatch_guids() {
        let payload = json!({
            "status": {
                "entityGUIDs": ["GUID-A", "GUID-B"]
            }
        });

        let client = setup_mock_client(Some(payload), None);
        let checker = get_checker(client);

        let err = checker.check_guid().unwrap_err();
        assert!(err.0.contains("Mismatching"));
    }

    #[test]
    fn test_fail_missing_status_field() {
        let payload = json!({
            "spec": { "some": "config" }
        });

        let client = setup_mock_client(Some(payload), None);
        let checker = get_checker(client);

        let err = checker.check_guid().unwrap_err();
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

        let err = checker.check_guid().unwrap_err();
        assert!(err.0.contains("empty"));
    }

    #[test]
    fn test_fail_resource_not_found() {
        let client = setup_mock_client(None, None);
        let checker = get_checker(client);

        let err = checker.check_guid().unwrap_err();
        assert!(err.0.contains("not found"));
    }

    #[test]
    fn test_fail_k8s_client_error() {
        let client = setup_mock_client(None, Some("Connection refused".to_string()));
        let checker = get_checker(client);

        let err = checker.check_guid().unwrap_err();
        assert!(err.0.contains("K8s error"));
        assert!(err.0.contains("Connection refused"));
    }
}
