use crate::agent_control::config::{
    default_group_version_kinds, helmrelease_v2_type_meta, helmrepository_type_meta,
};
use crate::cli::errors::CliError;
use crate::cli::install_agent_control::REPOSITORY_NAME;
use crate::cli::utils::{retry, try_new_k8s_client};
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::k8s::labels::Labels;
use clap::Parser;
use either::Either;
use kube::api::{DynamicObject, ObjectList, TypeMeta};
use kube::client::Status;
use std::fmt::Debug;
use std::time::Duration;
use tracing::info;

#[derive(Debug, Parser)]
pub struct AgentControlUninstallData {
    /// Release name
    #[arg(long)]
    pub release_name: String,
}

pub fn uninstall_agent_control(
    data: AgentControlUninstallData,
    namespace: String,
) -> Result<(), CliError> {
    let k8s_client = try_new_k8s_client(namespace.clone())?;
    let kinds_available = retrieve_api_resources(&k8s_client)?;

    // We want to make sure to delete first the AC so that it does not interfere with the deletion of the remaining resources.
    delete_agent_control_crs(&k8s_client, &kinds_available, data)?;
    // Deleting remaining objects owned by AC
    delete_owned_objects(&k8s_client, &kinds_available)?;

    Ok(())
}

fn retrieve_api_resources(k8s_client: &SyncK8sClient) -> Result<Vec<String>, CliError> {
    Ok(k8s_client
        .list_api_resources()
        .map_err(|err| CliError::K8sClient(format!("failed to retrieve api_resources: {err}")))?
        .resources
        .iter()
        .map(|r| r.kind.clone())
        .collect())
}

fn delete_owned_objects(
    k8s_client: &SyncK8sClient,
    kinds_available: &[String],
) -> Result<(), CliError> {
    let ac_owned_label_selector = Labels::default().selector();
    let mut tm_to_delete = default_group_version_kinds();

    tm_to_delete.push(TypeMeta {
        api_version: "v1".to_string(),
        kind: "ConfigMap".to_string(),
    });

    // Deleting CR owned by AC
    // TODO right now we are not honoring the dynamic tm_meta option of the AC.
    for tm in tm_to_delete {
        if kinds_available.contains(&tm.kind) {
            retry(30, Duration::from_secs(10), || {
                let res = k8s_client
                    .delete_dynamic_object_collection(&tm, ac_owned_label_selector.as_str())
                    .map_err(|err| {
                        CliError::K8sClient(format!(
                            "failed to delete resources {:?}: {err}",
                            &tm.kind
                        ))
                    })?;
                if is_collection_deleted(&tm, res) {
                    return Ok(());
                }
                Err(CliError::DeletingResource(format!("{tm:?}")))
            })?;
        }
    }
    Ok(())
}

fn delete_agent_control_crs(
    k8s_client: &SyncK8sClient,
    kinds_available: &[String],
    data: AgentControlUninstallData,
) -> Result<(), CliError> {
    if kinds_available.contains(&helmrelease_v2_type_meta().kind.to_string()) {
        retry(30, Duration::from_secs(10), || {
            let res = k8s_client
                .delete_dynamic_object(&helmrelease_v2_type_meta(), data.release_name.as_str())
                .map_err(|err| {
                    CliError::K8sClient(format!("failed to delete AC helmRelease : {err}"))
                })?;
            if is_resource_deleted(res) {
                info!("AC HelmRelease deleted");
                return Ok(());
            }
            Err(CliError::DeletingResource("AC HelmRelease".into()))
        })?;
    }

    if kinds_available.contains(&helmrepository_type_meta().kind.to_string()) {
        retry(10, Duration::from_secs(30), || {
            let res = k8s_client
                .delete_dynamic_object(&helmrepository_type_meta(), REPOSITORY_NAME)
                .map_err(|err| {
                    CliError::K8sClient(format!("failed to delete AC helmRepository  : {err}"))
                })?;
            if is_resource_deleted(res) {
                info!("AC HelmRepository deleted");
                return Ok(());
            }
            Err(CliError::DeletingResource("AC HelmRepository".into()))
        })?;
    }
    Ok(())
}

fn is_collection_deleted(tm: &TypeMeta, res: Either<ObjectList<DynamicObject>, Status>) -> bool {
    match res {
        Either::Left(l) => {
            if l.items.is_empty() {
                info!("Resources of type {:?} deleted", tm.kind);
                return true;
            }
            false
        }
        Either::Right(_) => {
            info!("Resources of type {:?} deleted", tm.kind);
            true
        }
    }
}

fn is_resource_deleted(res: Either<DynamicObject, Status>) -> bool {
    match res {
        Either::Left(_) => false,
        Either::Right(_) => true,
    }
}
