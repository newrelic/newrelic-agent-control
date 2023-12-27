use crate::config::agent_type::runtime_config::K8sObject;
use crate::config::super_agent_configs::AgentID;
use crate::k8s::error::K8sError;
use futures::executor::block_on;
use k8s_openapi::serde_json;
use kube::{
    api::DynamicObject,
    core::{ObjectMeta, TypeMeta},
};

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
            .map(|k8s_obj| self.create_dynamic_object(k8s_obj))
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

    fn create_dynamic_object(&self, k8s_obj: &K8sObject) -> Result<DynamicObject, SupervisorError> {
        let types = TypeMeta {
            api_version: k8s_obj.api_version.clone(),
            kind: k8s_obj.kind.clone(),
        };

        let metadata = ObjectMeta {
            name: Some(self.agent_id.to_string()),
            namespace: Some(self.executor.default_namespace().to_string()),
            ..Default::default()
        };

        let data = serde_json::to_value(&k8s_obj.fields).map_err(|e| {
            SupervisorError::ConfigError(format!("Error serializing fields: {}", e))
        })?;

        Ok(DynamicObject {
            types: Some(types),
            metadata,
            data,
        })
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::config::agent_type::runtime_config::K8sObject;
    use crate::k8s::executor::MockK8sExecutor;
    use k8s_openapi::serde_json;
    use kube::core::{ObjectMeta, TypeMeta};
    use serde_json::json;
    use std::collections::HashMap;

    const TEST_API_VERSION: &str = "test/v1";
    const TEST_KIND: &str = "test";

    fn test_dynamic_object() -> DynamicObject {
        DynamicObject {
            types: Some(TypeMeta {
                api_version: TEST_API_VERSION.to_string(),
                kind: TEST_KIND.to_string(),
            }),
            metadata: ObjectMeta {
                name: Some("test".to_string()),
                ..Default::default()
            },
            data: json!({}),
        }
    }

    fn test_equal_k8s_objects() -> HashMap<String, K8sObject> {
        let cr = K8sObject {
            api_version: test_dynamic_object().types.unwrap().api_version,
            kind: test_dynamic_object().types.unwrap().kind,
            metadata: None,
            ..Default::default()
        };
        HashMap::from([
            ("mock_cr1".to_string(), cr.clone()),
            ("mock_cr2".to_string(), cr.clone()),
        ])
    }

    #[test]
    fn test_supervisor_apply() {
        let mut mock_executor = MockK8sExecutor::default();

        mock_executor
            .expect_has_dynamic_object_changed()
            .times(2)
            .returning(|_| Ok(true));
        mock_executor
            .expect_default_namespace()
            .return_const("default".to_string());

        mock_executor
            .expect_apply_dynamic_object()
            .times(2)
            .withf(|dynamic_object| {
                dynamic_object.types.as_ref().unwrap().kind == TEST_KIND
                    && dynamic_object.types.as_ref().unwrap().api_version == TEST_API_VERSION
            })
            .returning(|_| Ok(()));

        let supervisor = CRSupervisor::new(
            AgentID::new("test-agent").unwrap(),
            Arc::new(mock_executor),
            test_equal_k8s_objects(),
        );

        supervisor.apply().unwrap();
    }
}
