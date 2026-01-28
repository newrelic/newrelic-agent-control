use crate::agent_control::config::{
    default_group_version_kinds, helmrelease_v2_type_meta, helmrepository_type_meta,
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
use std::cmp::Ordering;
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
    // Deleting remaining objects owned by AC
    delete_owned_objects(&k8s_client, &kinds_available, namespace)?;
    // Deleting remaining objects owned by AC in the namespace_agents. for example the instrumentation CR.
    delete_owned_objects(&k8s_client, &kinds_available, namespace_agents)?;

    Ok(())
}

fn delete_owned_objects(
    k8s_client: &SyncK8sClient,
    kinds_available: &HashSet<TypeMeta>,
    namespace: &str,
) -> Result<(), K8sCliError> {
    let ac_owned_label_selector = Labels::default().selector();
    let deleter = Deleter::with_default_retry_setup(k8s_client);
    for tm in objects_to_delete(kinds_available) {
        deleter.delete_collection_with_retry(&tm, namespace, &ac_owned_label_selector)?;
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

    // Introduce an ad-hoc ordering to ensure:
    // 1. Deterministic results
    // 2. Instrumentation CRs go first (i.e. before the K8s operator if present)
    tm_to_delete.sort_by(instrumentations_first);
    tm_to_delete
}

fn instrumentations_first(a: &TypeMeta, b: &TypeMeta) -> Ordering {
    if a.kind == "Instrumentation" && b.kind == "Instrumentation" {
        a.api_version.cmp(&b.api_version)
    } else if a.kind == "Instrumentation" {
        Ordering::Less
    } else if b.kind == "Instrumentation" {
        Ordering::Greater
    } else if a.kind == b.kind {
        a.api_version.cmp(&b.api_version)
    } else {
        a.kind.cmp(&b.kind)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instrumentations_first() {
        let instrumentation = TypeMeta {
            api_version: "v1".to_string(),
            kind: "Instrumentation".to_string(),
        };
        let config_map = TypeMeta {
            api_version: "v1".to_string(),
            kind: "ConfigMap".to_string(),
        };
        let deployment = TypeMeta {
            api_version: "apps/v1".to_string(),
            kind: "Deployment".to_string(),
        };

        // Instrumentation should come before others
        assert_eq!(
            instrumentations_first(&instrumentation, &config_map),
            Ordering::Less
        );
        assert_eq!(
            instrumentations_first(&config_map, &instrumentation),
            Ordering::Greater
        );

        // Others should be sorted alphabetically by kind
        assert_eq!(
            instrumentations_first(&config_map, &deployment),
            Ordering::Less // "ConfigMap" < "Deployment"
        );
        assert_eq!(
            instrumentations_first(&deployment, &config_map),
            Ordering::Greater
        );

        // Same kind should be sorted by api_version
        let deployment_v2 = TypeMeta {
            api_version: "apps/v2".to_string(),
            kind: "Deployment".to_string(),
        };
        assert_eq!(
            instrumentations_first(&deployment, &deployment_v2),
            Ordering::Less // "apps/v1" < "apps/v2"
        );
    }

    #[test]
    fn test_objects_to_delete_ordering_logic() {
        let mut list = [
            TypeMeta {
                api_version: "v1".to_string(),
                kind: "ConfigMap".to_string(),
            },
            TypeMeta {
                api_version: "apps/v1".to_string(),
                kind: "Deployment".to_string(),
            },
            TypeMeta {
                api_version: "v1".to_string(),
                kind: "Service".to_string(),
            },
            TypeMeta {
                api_version: "v1".to_string(),
                kind: "Instrumentation".to_string(),
            },
        ];

        list.sort_by(instrumentations_first);

        assert_eq!(list[0].kind, "Instrumentation");
        assert_eq!(list[1].kind, "ConfigMap");
        assert_eq!(list[2].kind, "Deployment");
        assert_eq!(list[3].kind, "Service");
    }

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn test_instrumentations_first_reflexivity(
            kind in prop_oneof!["Instrumentation", "[a-zA-Z0-9]+"],
            version in "[a-zA-Z0-9]+"
        ) {
            let tm = TypeMeta {
                kind,
                api_version: version,
            };
            // Comparison with itself MUST be Equal
            assert_eq!(instrumentations_first(&tm, &tm), Ordering::Equal);
        }

        #[test]
        fn test_instrumentations_always_first(
            mut list in proptest::collection::vec(
                (
                    prop_oneof![Just("Instrumentation".to_string()), "[a-zA-Z0-9]+"],
                    "[a-zA-Z0-9]+"
                ).prop_map(|(kind, api_version)| TypeMeta { kind, api_version }),
                0..50
            )
        ) {
            list.sort_by(instrumentations_first);

            let mut seen_non_instrumentation = false;
            for item in list {
                if item.kind == "Instrumentation" {
                    prop_assert!(!seen_non_instrumentation, "Found Instrumentation after a non-Instrumentation item");
                } else {
                    seen_non_instrumentation = true;
                }
            }
        }
    }
}
