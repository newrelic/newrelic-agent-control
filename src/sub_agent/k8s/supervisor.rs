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

pub trait SupervisorTrait {
    fn start(&self) -> Result<(), SupervisorError>;
    fn stop(&self) -> Result<(), SupervisorError>;
}

pub struct Supervisor<E>
where
    E: K8sDynamicObjectsManager + Send + Sync + 'static,
{
    executor: Arc<Mutex<E>>,
    created_resources: Arc<Mutex<Vec<(K8sResourceType, String)>>>,
}

impl<E> Supervisor<E>
where
    E: K8sDynamicObjectsManager + Send + Sync,
{
    pub fn new(executor: Arc<Mutex<E>>) -> Self {
        Self {
            executor,
            created_resources: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl<E> SupervisorTrait for Supervisor<E>
where
    E: K8sDynamicObjectsManager + Send + Sync,
{
    fn start(&self) -> Result<(), SupervisorError> {
        let resources = [
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
        ];

        for (resource_type, resource_name, cr_spec) in resources {
            let gvk = resource_type.to_gvk();
            let create_result = {
                let executor = self
                    .executor
                    .lock()
                    .map_err(|_| SupervisorError::LockError)?;
                block_on(executor.create_dynamic_object(gvk, cr_spec))
            };

            if let Err(err) = create_result {
                error!(
                    "Error creating CR: {} for resource type: {:?}, Error: {:?}",
                    resource_name, resource_type, err
                );
                continue;
            }

            let mut created_resources = self
                .created_resources
                .lock()
                .map_err(|_| SupervisorError::LockError)?;
            created_resources.push((resource_type, resource_name.to_string()));
        }

        info!("K8sSupervisor started and CRs created");
        Ok(())
    }

    fn stop(&self) -> Result<(), SupervisorError> {
        let resources_to_remove = {
            let resources = self
                .created_resources
                .lock()
                .map_err(|_| SupervisorError::LockError)?;
            resources.clone()
        };

        for (resource_type, resource_name) in resources_to_remove {
            let gvk = resource_type.to_gvk();
            let delete_result = {
                let executor = self
                    .executor
                    .lock()
                    .map_err(|_| SupervisorError::LockError)?;
                block_on(executor.delete_dynamic_object(gvk, &resource_name))
            };

            if let Err(err) = delete_result {
                error!(
                    "Error deleting resource: {}, type: {:?}, Error: {:?}",
                    resource_name, resource_type, err
                );
            }
        }

        let mut resources = self
            .created_resources
            .lock()
            .map_err(|_| SupervisorError::LockError)?;
        resources.clear();

        info!("K8sSupervisor stopped and CRs deleted");
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use crate::k8s::Error;
    use async_trait::async_trait;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use kube::core::{DynamicObject, GroupVersionKind, TypeMeta};
    use mockall::{mock, predicate};
    use serde_json::json;

    mock! {
        pub K8sExecutorMock {}

        #[async_trait]
        impl K8sDynamicObjectsManager for K8sExecutorMock {
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
        let mut mock_executor = MockK8sExecutorMock::new();

        // Mock the behavior for creating dynamic objects
        mock_executor
            .expect_create_dynamic_object()
            .with(predicate::always(), predicate::always())
            .times(2)
            .returning(|_, _| Ok(create_mock_dynamic_object()));

        let supervisor = Supervisor::new(Arc::new(Mutex::new(mock_executor)));
        let start_result = supervisor.start();

        assert!(start_result.is_ok());

        // Check if resources are added correctly
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

    #[test]
    fn test_supervisor_stop() {
        let mut mock_executor = MockK8sExecutorMock::new();

        // Mock behavior for deleting dynamic objects
        mock_executor
            .expect_delete_dynamic_object()
            .with(predicate::always(), predicate::always())
            .times(2)
            .returning(|_, _| Ok(()));

        let supervisor = Supervisor::new(Arc::new(Mutex::new(mock_executor)));

        // Simulate resources being created
        supervisor.created_resources.lock().unwrap().push((
            K8sResourceType::OtelHelmRepository,
            "open-telemetry".to_string(),
        ));
        supervisor.created_resources.lock().unwrap().push((
            K8sResourceType::OtelColHelmRelease,
            "otel-collector".to_string(),
        ));

        let stop_result = supervisor.stop();
        assert!(stop_result.is_ok());

        // Ensure that created_resources is empty after stop
        assert!(supervisor.created_resources.lock().unwrap().is_empty());
    }
}
