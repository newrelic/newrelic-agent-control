use crate::agent_control::config::{
    default_group_version_kinds, helmrelease_v2_type_meta, helmrepository_type_meta,
    instrumentation_v1beta3_type_meta,
};
use crate::cli::k8s::errors::K8sCliError;
use crate::cli::k8s::install::agent_control::REPOSITORY_NAME;
use crate::cli::k8s::uninstall::Deleter;
use crate::cli::k8s::utils::{retrieve_api_resources, try_new_k8s_client};
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::k8s::labels::Labels;
use clap::Parser;
use kube::api::TypeMeta;
use std::collections::HashSet;

#[derive(Debug, Clone, Parser)]
pub struct AgentControlUninstallData {
    /// namespace were the agent control agents were running
    #[arg(long)]
    pub namespace_agents: String,

    /// Name of the Helm release
    #[arg(long)]
    pub release_name: String,
}

pub fn uninstall_agent_control(
    namespace: &str,
    uninstall_data: &AgentControlUninstallData,
) -> Result<(), K8sCliError> {
    let k8s_client = try_new_k8s_client()?;
    let kinds_available = retrieve_api_resources(&k8s_client)?;
    let AgentControlUninstallData {
        namespace_agents,
        release_name,
    } = uninstall_data;

    // we delete first the AC so that it does not interfere (by recreating resources that we have just deleted).
    delete_agent_control_crs(&k8s_client, &kinds_available, namespace, release_name)?;

    // We filter the static list of objects we want to delete against what is actually available in the cluster.
    let valid_objects_to_delete = objects_to_delete(&kinds_available);

    // We need to handle the Instrumentation resources separately because we order the resource
    // deletion like this:
    // 1. Owned objects on the AC namespace. This deletes the Operator HelmRelease first, which
    //    ends up removing the Instrumentation CRD.
    // 2. Owned objects on the Agents namespace, which includes the Instrumentation CR that we
    //    had listed before the deletions started.
    // 3. The Instrumentation deletion attempts, coming after the Operator is out, will fail as
    //    its Resource API no longer exists at this point.
    let instrumentations_filter = [instrumentation_v1beta3_type_meta()];
    let (instrumentations_only, no_instrumentations): (Vec<_>, Vec<_>) = valid_objects_to_delete
        .into_iter()
        .partition(|tm| instrumentations_filter.contains(tm));

    // Operating over Instrumentations only.
    delete_owned_objects(&k8s_client, &instrumentations_only, namespace)?;
    delete_owned_objects(&k8s_client, &instrumentations_only, namespace_agents)?;

    // Operating over everything else.
    delete_owned_objects(&k8s_client, &no_instrumentations, namespace)?;
    delete_owned_objects(&k8s_client, &no_instrumentations, namespace_agents)?;

    Ok(())
}

fn delete_owned_objects(
    k8s_client: &SyncK8sClient,
    objects_to_delete: &[TypeMeta],
    namespace: &str,
) -> Result<(), K8sCliError> {
    let ac_owned_label_selector = Labels::default().selector();
    let deleter = Deleter::with_default_retry_setup(k8s_client);
    for tm in objects_to_delete {
        deleter.delete_collection_with_retry(tm, namespace, &ac_owned_label_selector)?;
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
    namespace: &str,
    release_name: &str,
) -> Result<(), K8sCliError> {
    let mut crs_to_delete: Vec<(TypeMeta, &str)> = vec![
        (helmrelease_v2_type_meta(), release_name),
        (helmrepository_type_meta(), REPOSITORY_NAME),
    ];

    crs_to_delete.retain(|(tm, _)| kinds_available.contains(tm));

    let deleter = Deleter::with_default_retry_setup(k8s_client);
    for (tm, object_name) in crs_to_delete {
        deleter.delete_object_with_retry(&tm, object_name, namespace)?;
    }

    Ok(())
}
