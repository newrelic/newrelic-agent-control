use futures::executor::block_on;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tracing::{error, info};

use crate::k8s::executor::{K8sDynamicObjectsManager, K8sResourceType};
use crate::sub_agent::k8s::sample_crs::{OTELCOL_HELM_RELEASE_CR, OTEL_HELM_REPOSITORY_CR};

#[derive(Debug, Error)]
pub enum SupervisorError {
    #[error("K8s supervisor lock error")]
    LockError,
}

pub struct Supervisor<E: K8sDynamicObjectsManager + Send + Sync> {
    executor: Arc<Mutex<E>>,
    created_resources: Arc<Mutex<Vec<(K8sResourceType, String)>>>,
}

impl<E: K8sDynamicObjectsManager + Send + Sync + 'static> Supervisor<E> {
    pub fn new(executor: Arc<Mutex<E>>) -> Self {
        Self {
            executor,
            created_resources: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub async fn start(&self) -> Result<(), SupervisorError> {
        let executor_guard = self
            .executor
            .lock()
            .map_err(|_| SupervisorError::LockError)?;
        let mut created_resources_guard = self
            .created_resources
            .lock()
            .map_err(|_| SupervisorError::LockError)?;

        for resource in vec![
            (
                K8sResourceType::OtelHelmRepository,
                "open-telemetry",
                OTEL_HELM_REPOSITORY_CR,
            ),
            (
                K8sResourceType::OtelColHelmRelease,
                "otel-collector",
                OTELCOL_HELM_RELEASE_CR,
            ),
        ] {
            let (resource_type, resource_name, cr_spec) = resource;
            let gvk = resource_type.to_gvk();
            match block_on(executor_guard.create_dynamic_object(gvk, cr_spec)) {
                Ok(_) => created_resources_guard.push((resource_type, resource_name.to_string())),
                Err(err) => error!(
                    "Error creating CR: {} for resource type: {:?}, Error: {:?}",
                    resource_name, resource_type, err
                ),
            }
        }

        info!("K8sSupervisor started and CRs created");
        Ok(())
    }

    pub async fn stop(&self) -> Result<(), SupervisorError> {
        let executor_guard = self
            .executor
            .lock()
            .map_err(|_| SupervisorError::LockError)?;
        let mut resources_guard = self
            .created_resources
            .lock()
            .map_err(|_| SupervisorError::LockError)?;

        let mut resources_to_remove = vec![];

        for (resource_type, resource_name) in resources_guard.iter() {
            let gvk = resource_type.to_gvk();
            match block_on(executor_guard.delete_dynamic_object(gvk, resource_name)) {
                Ok(_) => resources_to_remove.push((resource_type.clone(), resource_name.clone())),
                Err(err) => error!(
                    "Error deleting resource: {}, type: {:?}, Error: {:?}",
                    resource_name, resource_type, err
                ),
            }
        }

        // Remove the deleted resources from the original vector
        resources_guard.retain(|resource| !resources_to_remove.contains(resource));

        info!("K8sSupervisor stopped and CRs deleted");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::k8s::Error;
    use async_trait::async_trait;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use kube::core::{DynamicObject, GroupVersionKind};

    use mockall::{mock, predicate};

    mock! {
        K8sExecutorTrait {}

        #[async_trait]
        impl K8sDynamicObjectsManager for K8sExecutorTrait {
            async fn create_dynamic_object(
                &self,
                gvk: GroupVersionKind,
                spec: &str,
            ) -> Result<DynamicObject, Error>;

            async fn delete_dynamic_object(
                &self,
                gvk: GroupVersionKind,
                name: &str,
            ) -> Result<(), Error>;
        }
    }

    #[tokio::test]
    async fn test_supervisor_start() {
        let mut mock_executor = MockK8sExecutorTrait::new();

        // Mock the behavior for creating dynamic objects
        mock_executor
            .expect_create_dynamic_object()
            .with(predicate::always(), predicate::always())
            .times(2)
            .returning(|_, _| {
                Ok(DynamicObject {
                    types: Default::default(),
                    metadata: ObjectMeta::default(),
                    data: serde_json::Value::Null,
                })
            });

        let supervisor = Supervisor::new(Arc::new(Mutex::new(mock_executor)));
        let start_result = supervisor.start().await;

        assert!(start_result.is_ok());

        // Lock the created_resources to access its state
        let created_resources_guard = supervisor.created_resources.lock().unwrap();
        assert_eq!(created_resources_guard.len(), 2);
        assert!(created_resources_guard.contains(&(
            K8sResourceType::OtelHelmRepository,
            "open-telemetry".to_string()
        )));
        assert!(created_resources_guard.contains(&(
            K8sResourceType::OtelColHelmRelease,
            "otel-collector".to_string()
        )));
    }

    #[tokio::test]
    async fn test_supervisor_stop() {
        let mut mock_executor = MockK8sExecutorTrait::new();

        // Mock the behavior for deleting dynamic objects
        mock_executor
            .expect_delete_dynamic_object()
            .with(predicate::always(), predicate::always())
            .times(2)
            .returning(|_, _| Ok(()));

        let mut supervisor = Supervisor::new(Arc::new(Mutex::new(mock_executor)));

        // Manually add resources to `created_resources` to simulate the start process
        {
            let mut created_resources_guard = supervisor.created_resources.lock().unwrap();
            created_resources_guard.push((
                K8sResourceType::OtelHelmRepository,
                "open-telemetry".to_string(),
            ));
            created_resources_guard.push((
                K8sResourceType::OtelColHelmRelease,
                "otel-collector".to_string(),
            ));
        }

        let stop_result = supervisor.stop().await;
        assert!(stop_result.is_ok());

        // Ensure that `created_resources` is empty after stop
        assert!(supervisor.created_resources.lock().unwrap().is_empty());
    }
}
