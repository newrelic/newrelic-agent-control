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
    k8s::labels::AGENT_CONTROL_VERSION_SET_FROM,
};

/// Implementation of [`DynamicObjectListBuilder`] for generating the dynamic object lists corresponding to the Agent Control resources.
///
/// To be applied via [`install_or_upgrade`](super::install_or_upgrade).
pub struct InstallFlux;

pub const AGENT_CONTROL_CD_RELEASE_NAME: &str = "agent-control-cd";
const CHART_NAME: &str = "agent-control-cd";
pub const HELM_RELEASE_NAME: &str = CHART_NAME;
pub const HELM_REPOSITORY_NAME: &str = CHART_NAME;

impl DynamicObjectListBuilder for InstallFlux {
    // TODO this mostly duplicates the AgentControl implementation besides a few constants. Extracting to a function might be worth it.
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
            HELM_REPOSITORY_NAME,
            namespace,
            labels.clone(),
            BTreeMap::default(),
        );

        // This is not strictly necessary, but it helps to ensure that the labels are consistent
        let mut helm_release_labels = labels;
        helm_release_labels.insert(AGENT_CONTROL_VERSION_SET_FROM.to_string(), source);

        let helm_release_obj_meta_data = obj_meta_data(
            HELM_RELEASE_NAME,
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
                HELM_REPOSITORY_NAME,
                version.as_str(),
                &data.chart_name,
                helm_release_obj_meta_data,
            ),
        ]
    }
}
