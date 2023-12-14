use crate::config::agent_type::runtime_config::K8sObject;
use crate::config::super_agent_configs::AgentID;
use crate::k8s::error::K8sError;
use futures::executor::block_on;
use kube::api::DynamicObject;
use kube::ResourceExt;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tracing::{error, info};

#[cfg_attr(test, mockall_double::double)]
use crate::k8s::executor::K8sExecutor;

#[derive(Debug, Error)]
pub enum SupervisorError {
    #[error("applying k8s resource {0}")]
    ApplyError(String),

    #[error("the kube client returned an error: `{0}`")]
    Generic(#[from] K8sError),

    #[error("applying k8s resource {0}")]
    ConfigError(String),
}

/// CRSupervisor - Supervises Kubernetes resources.
/// To be considered:
/// - Start function hardcodes resources; needs dynamic definition once we add the configuration.
/// - Uses shared executor via Arc; consider design implications about sharing executor through all the supervisors.
/// - RefCell for internal mutability; it might change depending on future implementations.
/// - Synchronous block_on operations; review async handling.

pub struct CRSupervisor {
    agent_id: AgentID,
    executor: Arc<K8sExecutor>,
    k8s_objects: HashMap<String, K8sObject>,
}

impl CRSupervisor {
    pub fn new(
        agent_id: AgentID,
        executor: Arc<K8sExecutor>,
        k8s_objects: HashMap<String, K8sObject>,
    ) -> Self {
        Self {
            agent_id,
            executor,
            k8s_objects,
        }
    }

    pub fn apply(&self) -> Result<(), SupervisorError> {
        let resources = self.build_dynamic_objects()?;
        for res in resources {
            block_on(self.apply_k8s_resource(&res))?;
        }

        info!("K8sSupervisor started and CRs created");
        Ok(())
    }

    fn build_dynamic_objects(&self) -> Result<Vec<DynamicObject>, SupervisorError> {
        self.k8s_objects
            .values()
            .map(|k8s_obj| self.executor.create_dynamic_object(&self.agent_id, k8s_obj))
            .collect()
    }

    async fn apply_k8s_resource(&self, obj: &DynamicObject) -> Result<(), SupervisorError> {
        if !self.executor.has_dynamic_object_changed(obj).await? {
            return Ok(());
        }

        self.executor
            .apply_dynamic_object(obj)
            .await
            .map_err(|e| SupervisorError::ApplyError(format!("applying dynamic object: {}", e)))
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::config::agent_type::runtime_config::K8sObject;
    use crate::k8s::executor::MockK8sExecutor;
    use k8s_openapi::serde_json;
    use kube::core::{ObjectMeta, TypeMeta};
    use std::collections::HashMap;

    pub fn create_mock_dynamic_object() -> DynamicObject {
        DynamicObject {
            types: Some(TypeMeta {
                api_version: "v1".into(),
                kind: "MockKind".into(),
            }),
            metadata: ObjectMeta {
                name: Some("test".to_string()),
                ..Default::default()
            },
            data: serde_json::json!({}),
        }
    }

    pub fn create_mock_k8s_objects(api_version: &str, kind: &str) -> HashMap<String, K8sObject> {
        let mock_k8s_obj = K8sObject {
            api_version: api_version.to_string(),
            kind: kind.to_string(),
            metadata: None,
            fields: serde_yaml::Mapping::default(),
        };
        let mut k8s_objects = HashMap::new();
        k8s_objects.insert("test".to_string(), mock_k8s_obj);
        k8s_objects
    }

    #[test]
    fn test_supervisor_already_started() {
        let mut mock_executor = MockK8sExecutor::default();

        mock_executor
            .expect_has_dynamic_object_changed()
            .times(1)
            .returning(|_| Ok(false));

        mock_executor
            .expect_create_dynamic_object()
            .withf(|agent_id, k8s_obj| {
                agent_id.to_string() == "test-agent" && k8s_obj.kind == "MockKind"
            })
            .times(1)
            .returning(|_, _| Ok(create_mock_dynamic_object()));

        let mut supervisor = CRSupervisor::new(
            AgentID::new("test-agent").unwrap(),
            Arc::new(mock_executor),
            create_mock_k8s_objects("v1", "MockKind"),
        );

        let start_result = supervisor.apply();

        assert!(start_result.is_ok());
    }
}
