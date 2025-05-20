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
use std::collections::HashSet;
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

    // we delete first the AC so that it does not interfere (by recreating resources that we have just deleted).
    delete_agent_control_crs(&k8s_client, &kinds_available, data)?;
    // Deleting remaining objects owned by AC
    delete_owned_objects(&k8s_client, &kinds_available)?;

    Ok(())
}

fn retrieve_api_resources(k8s_client: &SyncK8sClient) -> Result<HashSet<TypeMeta>, CliError> {
    let mut tm_available = HashSet::new();

    let all_api_resource_list = k8s_client
        .list_api_resources()
        .map_err(|err| CliError::K8sClient(format!("failed to retrieve api_resources: {err}")))?;

    for api_resource_list in &all_api_resource_list {
        for resource in &api_resource_list.resources {
            tm_available.insert(TypeMeta {
                api_version: api_resource_list.group_version.clone(),
                kind: resource.kind.clone(),
            });
        }
    }

    Ok(tm_available)
}

fn delete_owned_objects(
    k8s_client: &SyncK8sClient,
    kinds_available: &HashSet<TypeMeta>,
) -> Result<(), CliError> {
    let ac_owned_label_selector = Labels::default().selector();

    for tm in objects_to_delete(kinds_available) {
        retry(30, Duration::from_secs(10), || {
            let res = k8s_client
                .delete_dynamic_object_collection(&tm, ac_owned_label_selector.as_str())
                .map_err(|err| {
                    CliError::K8sClient(format!(
                        "failed to delete resources {:?}: {}",
                        &tm.kind, err
                    ))
                })?;
            if is_collection_deleted(res) {
                info!("Resources of type {:?} deleted", tm.kind);
                return Ok(());
            }
            Err(CliError::DeleteResource(format!("{tm:?}")))
        })?;
    }
    Ok(())
}

// TODO right now we are not honoring the dynamic tm_meta option of the AC.
/// objects_to_delete retrieves the static list of object known by AC, ignoring any dynamic list.
/// Moreover, it adds ConfigMap to the list since it is not part of the default_group_version_kinds().
/// it also filters away object that are not available in the cluster.
fn objects_to_delete(kinds_available: &HashSet<TypeMeta>) -> Vec<TypeMeta> {
    let mut tm_to_delete = default_group_version_kinds();

    tm_to_delete.push(TypeMeta {
        api_version: "v1".to_string(),
        kind: "ConfigMap".to_string(),
    });

    tm_to_delete.retain(|tm| kinds_available.contains(tm));
    tm_to_delete
}

fn delete_agent_control_crs(
    k8s_client: &SyncK8sClient,
    kinds_available: &HashSet<TypeMeta>,
    data: AgentControlUninstallData,
) -> Result<(), CliError> {
    let mut crs_to_delete: Vec<(TypeMeta, &str)> = vec![
        (helmrelease_v2_type_meta(), data.release_name.as_str()),
        (helmrepository_type_meta(), REPOSITORY_NAME),
    ];

    info!("Deleting resources of type {:?}", kinds_available);

    crs_to_delete.retain(|(tm, _)| kinds_available.contains(tm));
    info!("Deleting resources of type {:?}", crs_to_delete);

    for (tm, object_name) in crs_to_delete {
        retry(30, Duration::from_secs(10), || {
            info!("Deleting resources of type {:?}", tm.kind);

            let res = k8s_client
                .delete_dynamic_object(&tm, object_name)
                .map_err(|err| {
                    CliError::K8sClient(format!(
                        "failed to delete resources {:?}: {}",
                        &tm.kind, err
                    ))
                })?;
            if is_resource_deleted(res) {
                info!("Resources of type {:?} deleted", tm.kind);
                return Ok(());
            }
            Err(CliError::DeleteResource(format!("{tm:?}")))
        })?;
    }

    Ok(())
}

fn is_collection_deleted(res: Either<ObjectList<DynamicObject>, Status>) -> bool {
    match res {
        Either::Left(l) => {
            if l.items.is_empty() {
                return true;
            }
            false
        }
        Either::Right(_) => true,
    }
}

fn is_resource_deleted(res: Either<DynamicObject, Status>) -> bool {
    match res {
        Either::Left(_) => false,
        Either::Right(_) => true,
    }
}
