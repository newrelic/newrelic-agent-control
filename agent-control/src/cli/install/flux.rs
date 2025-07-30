use std::collections::BTreeMap;

use kube::api::DynamicObject;
use tracing::debug;

use crate::{
    cli::{
        install::{
            DynamicObjectListBuilder, InstallData, get_local_or_remote_version, helm_release,
            helm_repository, obj_meta_data,
        },
        utils::parse_key_value_pairs,
    },
    k8s::labels::FLUX_VERSION_SET_FROM,
};

/// Implementation of [`DynamicObjectListBuilder`] for generating the dynamic object lists corresponding to the Agent Control resources.
///
/// To be applied via [`install_or_upgrade`](super::install_or_upgrade).
pub struct InstallFlux;

pub const RELEASE_NAME: &str = "flux2";
pub const REPOSITORY_NAME: &str = "flux";

impl DynamicObjectListBuilder for InstallFlux {
    // FIXME this mostly duplicates the AgentControl implementation besides a few constants. Extracting to a function might be worth it.
    fn build_dynamic_object_list(
        &self,
        namespace: &str,
        maybe_existing_helm_release: Option<&DynamicObject>,
        data: &InstallData,
    ) -> Vec<kube::api::DynamicObject> {
        let (version, source) =
            get_local_or_remote_version(maybe_existing_helm_release, data.chart_version.clone());

        let labels = data
            .extra_labels
            .as_ref()
            .map(parse_key_value_pairs)
            .unwrap_or_default();
        debug!("Parsed labels: {:?}", labels);

        let helm_repository_obj_meta_data = obj_meta_data(
            REPOSITORY_NAME,
            namespace,
            labels.clone(),
            BTreeMap::default(),
        );

        // This is not strictly necessary, but it helps to ensure that the labels are consistent
        let mut helm_release_labels = labels;
        helm_release_labels.insert(FLUX_VERSION_SET_FROM.to_string(), source);

        let helm_release_obj_meta_data = obj_meta_data(
            RELEASE_NAME,
            namespace,
            helm_release_labels,
            BTreeMap::default(),
        );

        vec![
            helm_repository(
                data.repository_url.as_str(),
                data.repository_secret_reference_name.clone(),
                data.repository_certificate_secret_reference_name.clone(),
                helm_repository_obj_meta_data,
            ),
            helm_release(
                &data.secrets,
                REPOSITORY_NAME,
                version.as_str(),
                &data.chart_name,
                helm_release_obj_meta_data,
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use kube::api::ObjectMeta;

    use crate::{
        agent_control::config::{helmrelease_v2_type_meta, helmrepository_type_meta},
        cli::install::REPOSITORY_URL,
        k8s::labels::LOCAL_VAL,
    };

    use super::*;

    const LOCAL_TEST_VERSION: &str = "1.0.0";
    const TEST_NAMESPACE: &str = "test-namespace";

    /*
    apiVersion: source.toolkit.fluxcd.io/v1
    kind: HelmRepository
    metadata:
      name: flux-repo
      namespace: default
    spec:
      interval: 1m
      url: https://fluxcd-community.github.io/helm-charts
    ---
    apiVersion: helm.toolkit.fluxcd.io/v2
    kind: HelmRelease
    metadata:
      name: flux2
    spec:
      interval: 1m
      chart:
        spec:
          sourceRef:
            kind: HelmRepository
            name: flux-repo
            namespace: default
          chart: flux2
          version: 2.15.0
      values:
        installCRDS: true
        sourceController:
          create: true
        helmController:
          create: true
        kustomizeController:
          create: false
        imageAutomationController:
          create: false
        imageReflectionController:
          create: false
        notificationController:
          create: false
    */

    fn flux2_repository_object() -> DynamicObject {
        DynamicObject {
            types: Some(helmrepository_type_meta()),
            metadata: ObjectMeta {
                name: Some(REPOSITORY_NAME.to_string()),
                namespace: Some(TEST_NAMESPACE.to_string()),
                labels: Some(BTreeMap::from_iter([
                  /* TODO check if needed at all.
                  "managed-by" ?
                   */
                ])),
                ..ObjectMeta::default()
            },
            data: serde_json::json!({
              "spec": {
                "url": REPOSITORY_URL,
                "interval": "1m",
              }
            }),
        }
    }

    fn flux2_release_object(version: &str, source: &str) -> DynamicObject {
        DynamicObject {
            types: Some(helmrelease_v2_type_meta()),
            metadata: ObjectMeta {
                name: Some(RELEASE_NAME.to_string()),
                namespace: Some(TEST_NAMESPACE.to_string()),
                labels: Some(BTreeMap::from_iter([
                    /* TODO check if needed at all.
                    "managed-by" ?
                     */
                    (FLUX_VERSION_SET_FROM.to_string(), source.to_string()),
                ])),
                ..ObjectMeta::default()
            },
            data: serde_json::json!({
              "spec": {
                "interval": "1m",
                "chart": {
                  "spec": {
                    "sourceRef": {
                      "kind": "HelmRepository",
                      "name": REPOSITORY_NAME,
                      "namespace": TEST_NAMESPACE,
                    },
                    "chart": RELEASE_NAME,
                    "version": version,
                  }
                },
                "values": {
                  "installCRDS": true,
                  "sourceController": { "create": true },
                  "helmController": { "create": true },
                  "kustomizeController": { "create": false },
                  "imageAutomationController": { "create": false },
                  "imageReflectionController": { "create": false },
                  "notificationController": { "create": false },
                }
              }
            }),
        }
    }

    #[test]
    fn test_existing_object_no_label() {
        let dynamic_objects = InstallFlux.build_dynamic_object_list(
            TEST_NAMESPACE,
            Some(&DynamicObject {
                types: None,
                metadata: ObjectMeta::default(),
                data: serde_json::json!({
                    "spec": {
                        "chart": {
                            "spec":{
                                "version": "1.2.3",
                            }
                        }
                    }
                }),
            }),
            &InstallData::default(),
        );
        assert_eq!(
            dynamic_objects,
            vec![
                flux2_repository_object(),
                flux2_release_object(LOCAL_TEST_VERSION, LOCAL_VAL)
            ]
        );
    }
}
