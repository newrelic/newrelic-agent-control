use futures::executor::block_on;
use kube::api::DynamicObject;
use std::sync::Arc;
use thiserror::Error;
use tracing::{error, info};

#[cfg_attr(test, mockall_double::double)]
use crate::k8s::executor::K8sExecutor;

#[derive(Debug, Error)]
pub enum SupervisorError {
    #[error("applying k8s resource {0}")]
    ApplyError(String),
}

/// CRSupervisor - Supervises Kubernetes resources.
/// To be considered:
/// - Start function hardcodes resources; needs dynamic definition once we add the configuration.
/// - Uses shared executor via Arc; consider design implications about sharing executor through all the supervisors.
/// - RefCell for internal mutability; it might change depending on future implementations.
/// - Synchronous block_on operations; review async handling.

pub struct CRSupervisor {
    executor: Arc<K8sExecutor>,
}

impl CRSupervisor {
    pub fn new(executor: Arc<K8sExecutor>) -> Self {
        Self { executor }
    }

    pub fn apply(&self, resources: &[DynamicObject]) -> Result<(), SupervisorError> {
        for res in resources {
            let create_result = block_on(self.apply_k8s_resource(res));
            if let Err(err) = create_result {
                error!("Error creating CR: {:?}", err);
                continue;
            }
        }

        info!("K8sSupervisor started and CRs created");
        Ok(())
    }

    async fn apply_k8s_resource(&self, res: &DynamicObject) -> Result<(), SupervisorError> {
        self.executor
            .apply_dynamic_object(res)
            .await
            .map_err(|e| SupervisorError::ApplyError(format!("applying dynamic object: {}", e)))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::k8s::executor::MockK8sExecutor;
    use crate::sub_agent::k8s::sample_crs::get_sample_resources;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use kube::core::{DynamicObject, TypeMeta};
    use mockall::predicate;
    use serde_json::json;

    fn create_mock_dynamic_object() -> DynamicObject {
        DynamicObject {
            types: Some(TypeMeta {
                api_version: "v1".into(),
                kind: "MockKind".into(),
            }),
            metadata: ObjectMeta::default(),
            data: json!({}),
        }
    }

    #[test]
    fn test_supervisor_start() {
        let mut mock_executor = MockK8sExecutor::default();

        // Mock the behavior for creating dynamic objects
        mock_executor
            .expect_apply_dynamic_object()
            .with(predicate::always())
            .times(2)
            .returning(|_| Ok(()));

        let supervisor = CRSupervisor::new(Arc::new(mock_executor));
        let start_result = supervisor.apply(get_sample_resources().as_slice());

        assert!(start_result.is_ok());
    }
}
