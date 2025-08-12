//! Holds the logic to remove the CRs corresponding to the flux installation while leaving the installation
//! untouched.
//!

use std::sync::Arc;
use std::time::Duration;

use kube::api::{DynamicObject, TypeMeta};
use serde_json::json;
use tracing::{debug, info};

use crate::agent_control::config::{
    helmchart_type_meta, helmrelease_v2_type_meta, helmrepository_type_meta,
};
use crate::cli::errors::CliError;
use crate::cli::install::flux::{HELM_RELEASE_NAME, HELM_REPOSITORY_NAME};
use crate::cli::uninstall::Deleter;
use crate::cli::utils::try_new_k8s_client;
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::utils::retry::retry;

const SUSPEND_CHECK_MAX_RETRIES: usize = 30;
const SUSPEND_CHECK_INTERVAL: Duration = Duration::from_secs(5);

/// Suspends the HelmRelease and then removes the HelmRelease, HelmChart and HelmRepository used to handle
/// the Flux installation in Agent Control.
pub fn remove_flux_crs(namespace: &str) -> Result<(), CliError> {
    let k8s_client = try_new_k8s_client()?;
    let helmrelease_type_meta = helmrelease_v2_type_meta();
    let helmrepository_type_meta = helmrepository_type_meta();

    let helmrelease = get_helmrelease(
        &k8s_client,
        &helmrelease_type_meta,
        HELM_RELEASE_NAME,
        namespace,
    )?;

    suspend_helmrelease(
        &k8s_client,
        HELM_RELEASE_NAME,
        namespace,
        &helmrelease_type_meta,
        &helmrelease,
        SUSPEND_CHECK_MAX_RETRIES,
        SUSPEND_CHECK_INTERVAL,
    )?;

    let deleter = Deleter::with_default_retry_setup(&k8s_client);
    deleter.delete_object_with_retry(&helmrelease_type_meta, HELM_RELEASE_NAME, namespace)?;
    delete_helmchart_object(&deleter, HELM_RELEASE_NAME, &helmrelease)?;
    deleter.delete_object_with_retry(&helmrepository_type_meta, HELM_REPOSITORY_NAME, namespace)?;

    Ok(())
}

/// Suspends the helm-release represented by the provided object and waits and verifies that the suspension
/// is effectively applied.
fn suspend_helmrelease(
    k8s_client: &SyncK8sClient,
    name: &str,
    namespace: &str,
    helmrelease_type_meta: &TypeMeta,
    helmrelease: &DynamicObject,
    suspend_check_max_retries: usize,
    suspend_check_interval: Duration,
) -> Result<(), CliError> {
    info!(helrelease_name = name, "Suspending HelmRelease");

    let patch = json!({"spec": {"suspend": true}});
    k8s_client
        .patch_dynamic_object(helmrelease_type_meta, name, namespace, patch)
        .map_err(|err| CliError::Generic(format!("could not suspend HelmRelease {name}: {err}")))?;
    debug!(
        helrelease_name = name,
        "Checking that the helm-release is effectively suspended"
    );

    retry(suspend_check_max_retries, suspend_check_interval, || {
        let current = get_helmrelease(k8s_client, helmrelease_type_meta, name, namespace)?;
        if is_helmrelease_updated_after_suspension(helmrelease, &current) {
            Ok(())
        } else {
            Err(CliError::Generic(format!(
                "Could not verify that HelmRelease {name} was effectively suspended"
            )))
        }
    })?;
    info!(helrelease_name = name, "HelmRelease suspended");
    Ok(())
}

/// Helper to handle errors and non-existence when obtaining a HelmRelease.
fn get_helmrelease(
    k8s_client: &SyncK8sClient,
    tm: &TypeMeta,
    name: &str,
    namespace: &str,
) -> Result<Arc<DynamicObject>, CliError> {
    k8s_client
        .get_dynamic_object(tm, name, namespace)
        .map_err(|err| CliError::GetResource(err.to_string()))?
        .ok_or_else(|| CliError::GetResource(format!("could not find HelmRerelease {name}")))
}

/// Checks if `current` is updated from `previous` considering the fields `.metadata.resource_version` and
/// `.status.observed_generation`.
fn is_helmrelease_updated_after_suspension(
    previous: &DynamicObject,
    current: &DynamicObject,
) -> bool {
    let previous_resource_version = &previous.metadata.resource_version;
    let current_resource_version = &current.metadata.resource_version;

    let previous_observed_generation = previous
        .data
        .get("status")
        .and_then(|v| v.get("observedGeneration"));
    let current_observed_generation = current
        .data
        .get("status")
        .and_then(|v| v.get("observedGeneration"));

    // Checking observed_generation might not be strictly necessary, but we assure that HelmController is aware of the
    // change. See: <https://github.com/fluxcd/flux2/issues/4282#issuecomment-3164552839>
    (previous_resource_version != current_resource_version)
        && (previous_observed_generation != current_observed_generation)
}

/// Deletes the HelmChart referenced in the provided HelmRelease if any.
fn delete_helmchart_object(
    deleter: &Deleter,
    helmrelease_name: &str,
    helmrelease: &DynamicObject,
) -> Result<(), CliError> {
    let Some((helmchart_namespace, helmchart_name)) = helmrelease
        .data
        .get("status")
        .and_then(|v| v.get("helmChart"))
        .and_then(|chart_ref| chart_ref.as_str())
        .and_then(|v| v.split_once("/"))
    else {
        info!(
            "There was no HelmChart referenced in \"{}\", skipping deletion",
            helmrelease_name,
        );
        return Ok(());
    };
    let tm = helmchart_type_meta();
    deleter.delete_object_with_retry(&tm, helmchart_name, helmchart_namespace)
}

#[cfg(test)]
mod tests {
    use crate::k8s::client::MockSyncK8sClient;
    use either::Either;
    use kube::{
        api::{DynamicObject, ObjectMeta},
        core::Status,
    };
    use mockall::{Sequence, predicate};
    use serde_json::json;

    use super::*;

    /// Minimum release object for testing purposes.
    fn testing_helmrelease(
        namespace: &str,
        suspend: bool,
        resource_version: &str,
        status: serde_json::Value,
    ) -> DynamicObject {
        DynamicObject {
            types: Some(helmrelease_v2_type_meta()),
            metadata: ObjectMeta {
                name: Some(HELM_RELEASE_NAME.to_string()),
                namespace: Some(namespace.to_string()),
                resource_version: Some(resource_version.to_string()),
                ..Default::default()
            },
            data: json!({
                "spec": {"suspend": suspend},
                "status": status,
            }),
        }
    }

    #[test]
    fn test_suspend_helmrelease() {
        let mut mock_k8s_client = MockSyncK8sClient::new();
        let helmrelease_type_meta = helmrelease_v2_type_meta();
        let namespace = "test-namespace";

        let helmrelease =
            testing_helmrelease(namespace, false, "1", json!({"observedGeneration": 1}));

        mock_k8s_client
            .expect_patch_dynamic_object()
            .with(
                predicate::eq(helmrelease_v2_type_meta()),
                predicate::eq(HELM_RELEASE_NAME),
                predicate::eq(namespace),
                predicate::eq(json!({"spec": {"suspend": true}})),
            )
            .returning(move |_, _, _, _| {
                Ok(testing_helmrelease(
                    namespace,
                    true,
                    "1",
                    json!({"observedGeneration": 1}),
                ))
            });

        // HelmRelease is updated after some delay
        let mut seq = Sequence::new();
        mock_k8s_client
            .expect_get_dynamic_object()
            .once()
            .in_sequence(&mut seq)
            .returning(move |_, _, _| {
                Ok(Some(Arc::new(testing_helmrelease(
                    namespace,
                    true,
                    "1",
                    json!({"observedGeneration": 1}),
                ))))
            });
        mock_k8s_client
            .expect_get_dynamic_object()
            .once()
            .in_sequence(&mut seq)
            .returning(move |_, _, _| {
                Ok(Some(Arc::new(testing_helmrelease(
                    namespace,
                    true,
                    "2",
                    json!({"observedGeneration": 1}),
                ))))
            });
        mock_k8s_client
            .expect_get_dynamic_object()
            .once()
            .in_sequence(&mut seq)
            .returning(move |_, _, _| {
                Ok(Some(Arc::new(testing_helmrelease(
                    namespace,
                    true,
                    "2",
                    json!({"observedGeneration": 2}),
                ))))
            });

        let result = suspend_helmrelease(
            &mock_k8s_client,
            HELM_RELEASE_NAME,
            namespace,
            &helmrelease_type_meta,
            &helmrelease,
            10,
            Duration::from_millis(10),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_delete_helmchart() {
        let namespace = "testing-namespace";

        let helmrelease = testing_helmrelease(
            namespace,
            false,
            "1",
            json!({"helmChart": "helm-chart-namespace/helm-chart-name"}),
        );

        let mut mock_k8s_client = MockSyncK8sClient::new();
        mock_k8s_client
            .expect_delete_dynamic_object()
            .with(
                predicate::eq(helmchart_type_meta()),
                predicate::eq("helm-chart-name"),
                predicate::eq("helm-chart-namespace"),
            )
            .once()
            .returning(|_, _, _| Ok(Either::Right(Status::success())));

        let deleter = Deleter {
            k8s_client: &mock_k8s_client,
            max_attempts: 10,
            interval: Duration::from_millis(10),
        };
        let result = delete_helmchart_object(&deleter, HELM_RELEASE_NAME, &helmrelease);
        assert!(result.is_ok());
    }
}
