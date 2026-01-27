use std::time::Duration;

use either::Either;
use kube::{
    api::{DynamicObject, TypeMeta},
    core::Status,
};
use tracing::{debug, info};

use super::errors::K8sCliError;
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::utils::retry::retry;
use serde_json::json;

pub mod agent_control;
pub mod flux;

const DELETER_DEFAULT_MAX_ATTEMPTS: usize = 30;
const DELETER_DEFAULT_INTERVAL: Duration = Duration::from_secs(10);

// Helper to remove k8s objects and collections.
struct Deleter<'a> {
    k8s_client: &'a SyncK8sClient,
    max_attempts: usize,
    interval: Duration,
    patch_finalizers: bool,
}

impl<'a> Deleter<'a> {
    fn new(k8s_client: &'a SyncK8sClient) -> Self {
        Self {
            k8s_client,
            max_attempts: DELETER_DEFAULT_MAX_ATTEMPTS,
            interval: DELETER_DEFAULT_INTERVAL,
            patch_finalizers: false,
        }
    }

    fn with_patch_finalizers(self, patch_finalizers: bool) -> Self {
        Self {
            patch_finalizers,
            ..self
        }
    }

    fn delete_object_with_retry(
        &self,
        tm: &TypeMeta,
        name: &str,
        namespace: &str,
    ) -> Result<(), K8sCliError> {
        info!(%name, type=tm.kind, "Deleting resource");
        retry(self.max_attempts, self.interval, || {
            let res = self
                .k8s_client
                .delete_dynamic_object(tm, name, namespace)
                .map_err(|err| {
                    K8sCliError::DeleteResource(format!(
                        "could not delete resource '{}' of type '{}': {}",
                        name, tm.kind, err
                    ))
                })?;
            if is_resource_deleted(&res) {
                info!(%name, type=tm.kind, "Resource deleted");
                Ok(())
            } else {
                Err(K8sCliError::DeleteResource(format!(
                    "deletion of resource '{}' of type '{}' is not complete",
                    name, tm.kind
                )))
            }
        })
    }

    fn delete_collection_with_retry(
        &self,
        tm: &TypeMeta,
        namespace: &str,
        selector: &str,
    ) -> Result<(), K8sCliError> {
        retry(self.max_attempts, self.interval, {
            let mut patched_finalizers = false;
            move || {
                info!(type=tm.kind, %selector, "Deleting resources");
                match self
                    .k8s_client
                    .delete_dynamic_object_collection(tm, namespace, selector)
                    .map_err(|e| {
                        K8sCliError::DeleteResource(format!(
                            "failed to delete resources of type '{}': {}",
                            tm.kind, e
                        ))
                    })? {
                    // Deleted resources
                    Either::Right(_) => {
                        info!(type=tm.kind, %selector, "Resources deleted");
                        Ok(())
                    }
                    // No remaining objects
                    Either::Left(l) if l.items.is_empty() => {
                        info!(type=tm.kind, %selector, "Resources deleted");
                        Ok(())
                    }
                    // Some objects still remain, patch finalizers and retry
                    Either::Left(l) => {
                        if self.patch_finalizers && !patched_finalizers {
                            debug!(type=tm.kind, "Patching finalizers for remaining resources");
                            for obj in &l.items {
                                if let Some(name) = &obj.metadata.name {
                                    self.remove_finalizers_if_needed(tm, obj, name, namespace)?;
                                }
                            }
                        }
                        patched_finalizers = true;
                        Err(K8sCliError::DeleteResource(format!(
                            "deletion of resources of type '{}' is not complete",
                            tm.kind
                        )))
                    }
                }
            }
        })
    }

    fn remove_finalizers_if_needed(
        &self,
        tm: &TypeMeta,
        obj: &DynamicObject,
        name: &str,
        namespace: &str,
    ) -> Result<(), K8sCliError> {
        if let Some(finalizers) = &obj.metadata.finalizers
            && !finalizers.is_empty()
        {
            info!(%name, type=tm.kind, "Removing finalizers to unblock deletion");
            let patch = json!({
                "metadata": {
                    "finalizers": null
                }
            });
            self.k8s_client
                .patch_dynamic_object(tm, name, namespace, patch)
                .map_err(|err| {
                    K8sCliError::Generic(format!(
                        "failed to remove finalizers for resource '{}': {}",
                        name, err
                    ))
                })?;
        }
        Ok(())
    }
}

fn is_resource_deleted(res: &Either<DynamicObject, Status>) -> bool {
    res.is_right()
}
