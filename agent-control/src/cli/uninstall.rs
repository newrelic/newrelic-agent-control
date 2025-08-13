use std::time::Duration;

use either::Either;
use kube::{
    api::{DynamicObject, ObjectList, TypeMeta},
    core::Status,
};
use tracing::info;

#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::{cli::errors::CliError, utils::retry::retry};

pub mod agent_control;
pub mod flux;

const DELETER_DEFAULT_MAX_ATTEMPTS: usize = 30;
const DELETER_DEFAULT_INTERVAL: Duration = Duration::from_secs(10);

// Helper to remove k8s objects and collections.
struct Deleter<'a> {
    k8s_client: &'a SyncK8sClient,
    max_attempts: usize,
    interval: Duration,
}

impl<'a> Deleter<'a> {
    fn with_default_retry_setup(k8s_client: &'a SyncK8sClient) -> Self {
        Self {
            k8s_client,
            max_attempts: DELETER_DEFAULT_MAX_ATTEMPTS,
            interval: DELETER_DEFAULT_INTERVAL,
        }
    }

    fn delete_object_with_retry(
        &self,
        tm: &TypeMeta,
        name: &str,
        namespace: &str,
    ) -> Result<(), CliError> {
        info!(%name, type=tm.kind, "Deleting resource");
        retry(self.max_attempts, self.interval, || {
            let res = self
                .k8s_client
                .delete_dynamic_object(tm, name, namespace)
                .map_err(|err| {
                    CliError::DeleteResource(format!(
                        "could not delete resource '{}' of type '{}': {}",
                        name, tm.kind, err
                    ))
                })?;
            if is_resource_deleted(res) {
                info!(%name, type=tm.kind, "Resource deleted");
                Ok(())
            } else {
                Err(CliError::DeleteResource(format!(
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
    ) -> Result<(), CliError> {
        retry(self.max_attempts, self.interval, || {
            info!(type=tm.kind, %selector, "Deleting resources");
            let res = self
                .k8s_client
                .delete_dynamic_object_collection(tm, namespace, selector)
                .map_err(|err| {
                    CliError::DeleteResource(format!(
                        "failed to delete resources of type '{}': {}",
                        tm.kind, err
                    ))
                })?;
            if is_collection_deleted(res) {
                info!(type=tm.kind, %selector, "Resources deleted");
                Ok(())
            } else {
                Err(CliError::DeleteResource(format!(
                    "deletion of resources of type '{}' is not complete",
                    tm.kind
                )))
            }
        })
    }
}

fn is_collection_deleted(res: Either<ObjectList<DynamicObject>, Status>) -> bool {
    match res {
        Either::Right(_) => true,
        Either::Left(l) if l.items.is_empty() => true,
        Either::Left(_) => false,
    }
}

fn is_resource_deleted(res: Either<DynamicObject, Status>) -> bool {
    res.is_right()
}
