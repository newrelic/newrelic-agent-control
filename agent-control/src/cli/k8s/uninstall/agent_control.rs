//! Uninstalls the Agent Control release and its owned resources from Kubernetes.
use crate::agent_control::config::{
    default_group_version_kinds, helmrelease_v2_type_meta, helmrepository_type_meta,
    instrumentation_v1beta3_type_meta,
};
use crate::cli::k8s::errors::K8sCliError;
use crate::cli::k8s::install::agent_control::REPOSITORY_NAME;
use crate::cli::k8s::uninstall::Deleter;
use crate::cli::k8s::utils::{retrieve_api_resources, try_new_k8s_client};
use crate::k8s::annotations;
use crate::k8s::client::K8sClient;
use crate::k8s::labels::{self, Labels};
use clap::Parser;
use kube::api::TypeMeta;
use std::collections::{BTreeMap, HashSet};
use tracing::{debug, info};

/// Arguments for the Agent Control uninstall command.
#[derive(Debug, Clone, Parser)]
pub struct AgentControlUninstallData {
    /// Namespace where the Agent Control agents were running.
    #[arg(long)]
    pub namespace_agents: String,

    /// Name of the Helm release. Omit to skip deleting the release's own HelmRelease/HelmRepository CRs.
    #[arg(long)]
    pub release_name: Option<String>,
}

/// Removes the Agent Control custom resources and all owned objects from the given namespaces.
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
    match release_name {
        Some(release_name) => {
            delete_agent_control_crs(&k8s_client, &kinds_available, namespace, release_name)?;
        }
        None => info!("No release name provided, skipping deletion of Agent Control own CRs"),
    }

    // We filter the static list of objects we want to delete against what is actually available in the cluster.
    let valid_objects_to_delete = objects_to_delete(&kinds_available);

    // We need to delete the `Instrumentation` objects first because the corresponding CRD is
    // created by another agent (the K8s Operator). If this agent is uninstalled before removing
    // the Instrumentation resources then these deletion attempts will fail.
    //
    // So we split the list of objects to delete into three groups: Instrumentations (deleted
    // first, as above), the Flux CRs (HelmRelease/HelmRepository), and everything else.
    let instrumentations_filter = [instrumentation_v1beta3_type_meta()];
    let (instrumentations_only, remaining): (Vec<_>, Vec<_>) = valid_objects_to_delete
        .into_iter()
        .partition(|tm| instrumentations_filter.contains(tm));

    // The Flux CRs are handled separately from the rest: Agent Control's own self-managed
    // HelmRelease/HelmRepository are removed first (`delete_agent_control_crs`) and only when the
    // release name is set. Otherwise, deletion could collide between bootstrap's uninstall
    // and deployment's uninstall.
    let flux_cr_filter = [helmrelease_v2_type_meta(), helmrepository_type_meta()];
    let (flux_crs, no_instrumentations): (Vec<_>, Vec<_>) = remaining
        .into_iter()
        .partition(|tm| flux_cr_filter.contains(tm));

    // Operating over Instrumentations only.
    delete_owned_objects(&k8s_client, &instrumentations_only, namespace)?;
    delete_owned_objects(&k8s_client, &instrumentations_only, namespace_agents)?;

    // Operating over Flux CRs, skipping Agent Control's own self-managed ones.
    delete_sub_agent_owned_objects(&k8s_client, &flux_crs, namespace)?;
    delete_sub_agent_owned_objects(&k8s_client, &flux_crs, namespace_agents)?;

    // Operating over everything else.
    delete_owned_objects(&k8s_client, &no_instrumentations, namespace)?;
    delete_owned_objects(&k8s_client, &no_instrumentations, namespace_agents)?;

    Ok(())
}

fn delete_owned_objects<C: K8sClient>(
    k8s_client: &C,
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

/// Deletes objects of the given types that Agent Control created on behalf of a sub-agent,
/// skipping any that are Agent Control's own.
fn delete_sub_agent_owned_objects<C: K8sClient>(
    k8s_client: &C,
    objects_to_delete: &[TypeMeta],
    namespace: &str,
) -> Result<(), K8sCliError> {
    let deleter = Deleter::with_default_retry_setup(k8s_client);
    for tm in objects_to_delete {
        let objects = k8s_client
            .list_dynamic_objects(tm, namespace)
            .map_err(|err| {
                K8sCliError::GetResource(format!(
                    "could not list resources of type '{}': {}",
                    tm.kind, err
                ))
            })?;

        for obj in objects {
            let empty_map = BTreeMap::new();
            let object_labels = obj.metadata.labels.as_ref().unwrap_or(&empty_map);
            if !labels::is_managed_by_agent_control(object_labels) {
                continue;
            }

            let object_annotations = obj.metadata.annotations.as_ref().unwrap_or(&empty_map);
            let Some(name) = obj.metadata.name.as_deref() else {
                continue;
            };
            if !annotations::is_owned_by_sub_agent(object_annotations) {
                debug!(%name, type = tm.kind, "skipping resource owned by Agent Control itself");
                continue;
            }

            deleter.delete_object_with_retry(tm, name, namespace)?;
        }
    }
    Ok(())
}

// TODO right now we are not honoring the dynamic tm_meta option of the AC.
/// objects_to_delete retrieves the static list of object known by AC, ignoring any dynamic list.
/// Moreover, it adds ConfigMap to the list since it is not part of the default_group_version_kinds().
/// it also filters away object that are not available in the cluster.
/// On top of it, in the fluxless scenarios it loads the HelmRelease and HelmRepository CRs to be deleted,
/// but it is a noop since they are not actually present in the cluster.
fn objects_to_delete(kinds_available: &HashSet<TypeMeta>) -> Vec<TypeMeta> {
    let mut tm_to_delete = default_group_version_kinds();

    tm_to_delete.push(TypeMeta {
        api_version: "v1".to_string(),
        kind: "ConfigMap".to_string(),
    });

    tm_to_delete.retain(|tm| kinds_available.contains(tm));
    tm_to_delete
}

fn delete_agent_control_crs<C: K8sClient>(
    k8s_client: &C,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::agent_id::AgentID;
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::k8s::annotations::Annotations;
    use crate::k8s::client::tests::MockK8sClient;
    use either::Either;
    use kube::api::{DynamicObject, ObjectMeta};
    use kube::core::Status;
    use std::sync::Arc;

    const TEST_NAMESPACE: &str = "test-namespace";

    fn dynamic_object(
        labels: Option<BTreeMap<String, String>>,
        annotations: Option<BTreeMap<String, String>>,
    ) -> Arc<DynamicObject> {
        Arc::new(DynamicObject {
            types: None,
            metadata: ObjectMeta {
                name: Some("some-release".to_string()),
                namespace: Some(TEST_NAMESPACE.to_string()),
                labels,
                annotations,
                ..Default::default()
            },
            data: serde_json::Value::Null,
        })
    }

    #[test]
    fn skips_agent_control_owned_object() {
        let obj = dynamic_object(
            Some(Labels::new(&AgentID::AgentControl).get()),
            Some(Annotations::new_agent_control_owned().get()),
        );

        let mut k8s_client = MockK8sClient::default();
        k8s_client
            .expect_list_dynamic_objects()
            .once()
            .returning(move |_, _| Ok(vec![obj.clone()]));
        k8s_client.expect_delete_dynamic_object().never();

        let tm = helmrelease_v2_type_meta();
        assert!(delete_sub_agent_owned_objects(&k8s_client, &[tm], TEST_NAMESPACE).is_ok());
    }

    #[test]
    fn deletes_sub_agent_owned_object() {
        let agent_id = AgentID::try_from("foo-agent").unwrap();
        let agent_type_id = AgentTypeID::try_from("newrelic/com.example.foo:0.0.1").unwrap();
        let obj = dynamic_object(
            Some(Labels::new(&agent_id).get()),
            Some(Annotations::new_sub_agent_owned_with_type(&agent_type_id).get()),
        );

        let mut k8s_client = MockK8sClient::default();
        k8s_client
            .expect_list_dynamic_objects()
            .once()
            .returning(move |_, _| Ok(vec![obj.clone()]));
        k8s_client
            .expect_delete_dynamic_object()
            .once()
            .returning(|_, _| Ok(Either::Right(Status::default())));

        let tm = helmrelease_v2_type_meta();
        assert!(delete_sub_agent_owned_objects(&k8s_client, &[tm], TEST_NAMESPACE).is_ok());
    }

    #[test]
    fn skips_object_without_managed_by_label() {
        let obj = dynamic_object(None, Some(Annotations::new_agent_control_owned().get()));

        let mut k8s_client = MockK8sClient::default();
        k8s_client
            .expect_list_dynamic_objects()
            .once()
            .returning(move |_, _| Ok(vec![obj.clone()]));
        k8s_client.expect_delete_dynamic_object().never();

        let tm = helmrelease_v2_type_meta();
        assert!(delete_sub_agent_owned_objects(&k8s_client, &[tm], TEST_NAMESPACE).is_ok());
    }

    #[test]
    fn skips_object_without_owned_by_annotation() {
        let obj = dynamic_object(Some(Labels::new(&AgentID::AgentControl).get()), None);

        let mut k8s_client = MockK8sClient::default();
        k8s_client
            .expect_list_dynamic_objects()
            .once()
            .returning(move |_, _| Ok(vec![obj.clone()]));
        k8s_client.expect_delete_dynamic_object().never();

        let tm = helmrelease_v2_type_meta();
        assert!(delete_sub_agent_owned_objects(&k8s_client, &[tm], TEST_NAMESPACE).is_ok());
    }
}
