use futures::executor::block_on;
use std::{cell::RefCell, rc::Rc, sync::Arc};
use thiserror::Error;
use tracing::{error, info};

use crate::k8s::executor::K8sResourceType;
use crate::sub_agent::k8s::sample_crs::{OTELCOL_HELM_RELEASE_CR, OTEL_HELM_REPOSITORY_CR};

#[cfg_attr(test, mockall_double::double)]
use crate::k8s::executor::K8sExecutor;

#[derive(Debug, Error)]
pub enum SupervisorError {}

/// CRSupervisor - Supervises Kubernetes resources.
/// To be considered:
/// - Start function hardcodes resources; needs dynamic definition once we add the configuration.
/// - Uses shared executor via Arc; consider design implications about sharing executor through all the supervisors.
/// - RefCell for internal mutability; it might change depending on future implementations.
/// - Synchronous block_on operations; review async handling.

pub struct CRSupervisor {
    executor: Arc<K8sExecutor>,
    created_resources: Rc<RefCell<Vec<(K8sResourceType, String)>>>,
}

impl CRSupervisor {
    pub fn new(executor: Arc<K8sExecutor>) -> Self {
        Self {
            executor,
            created_resources: Rc::new(RefCell::new(Vec::new())),
        }
    }

    pub fn apply(&self) -> Result<(), SupervisorError> {
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
            let create_result = block_on(self.executor.create_dynamic_object(gvk, cr_spec));

            if let Err(err) = create_result {
                error!(
                    "Error creating CR: {} for resource type: {:?}, Error: {:?}",
                    resource_name, resource_type, err
                );
                continue;
            }

            self.created_resources
                .borrow_mut()
                .push((resource_type, resource_name.to_string()));
        }

        info!("K8sSupervisor started and CRs created");
        Ok(())
    }

    pub fn delete(&self) -> Result<(), SupervisorError> {
        for (resource_type, resource_name) in self.created_resources.borrow().iter() {
            let gvk = resource_type.to_gvk();
            let delete_result = block_on(self.executor.delete_dynamic_object(gvk, resource_name));

            if let Err(err) = delete_result {
                error!(
                    "Error deleting resource: {}, type: {:?}, Error: {:?}",
                    resource_name, resource_type, err
                );
            }
        }

        self.created_resources.borrow_mut().clear();

        info!("K8sSupervisor stopped and CRs deleted");
        Ok(())
    }
}
#[cfg(test)]
mod test {
    use super::*;
    use crate::k8s::executor::MockK8sExecutor;
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
            .expect_create_dynamic_object()
            .with(predicate::always(), predicate::always())
            .times(2)
            .returning(|_, _| Ok(create_mock_dynamic_object()));

        let supervisor = CRSupervisor::new(Arc::new(mock_executor));

        let start_result = supervisor.apply();

        assert!(start_result.is_ok());

        // Check if resources are added correctly
        let created_resources_guard = supervisor.created_resources.borrow();
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
        let mut mock_executor = MockK8sExecutor::default();

        // Mock behavior for deleting dynamic objects
        mock_executor
            .expect_delete_dynamic_object()
            .with(predicate::always(), predicate::always())
            .times(2)
            .returning(|_, _| Ok(()));

        let supervisor = CRSupervisor::new(Arc::new(mock_executor));

        // Simulate resources being created
        supervisor.created_resources.borrow_mut().push((
            K8sResourceType::OtelHelmRepository,
            "open-telemetry".to_string(),
        ));
        supervisor.created_resources.borrow_mut().push((
            K8sResourceType::OtelColHelmRelease,
            "otel-collector".to_string(),
        ));

        let stop_result = supervisor.delete();
        assert!(stop_result.is_ok());

        // Ensure that created_resources is empty after stop
        assert!(supervisor.created_resources.borrow().is_empty());
    }
}
