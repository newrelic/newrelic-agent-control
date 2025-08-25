use kube::api::DynamicObject;
use tracing::debug;

use crate::{
    agent_control::defaults::AGENT_CONTROL_ID,
    cli::{
        install::{
            DynamicObjectListBuilder, InstallData, get_local_or_remote_version, helm_release,
            helm_repository, obj_meta_data,
        },
        utils::parse_key_value_pairs,
    },
    k8s::{
        annotations::Annotations,
        labels::{AGENT_CONTROL_VERSION_SET_FROM, Labels},
    },
    sub_agent::identity::AgentIdentity,
};

/// Implementation of [`DynamicObjectListBuilder`] for generating the dynamic object lists corresponding to the Agent Control resources.
///
/// To be applied via [`install_or_upgrade`](super::install_or_upgrade).
pub struct InstallAgentControl;

pub const AGENT_CONTROL_DEPLOYMENT_RELEASE_NAME: &str = "agent-control-deployment";
pub const REPOSITORY_NAME: &str = AGENT_CONTROL_ID;

impl DynamicObjectListBuilder for InstallAgentControl {
    fn build_dynamic_object_list(
        &self,
        namespace: &str,
        release_name: &str,
        maybe_existing_helm_release: Option<&DynamicObject>,
        data: &InstallData,
    ) -> Vec<DynamicObject> {
        let (version, source) =
            get_local_or_remote_version(maybe_existing_helm_release, data.chart_version.clone());

        let agent_identity = AgentIdentity::new_agent_control_identity();

        let mut labels = Labels::new(&agent_identity.id);
        let extra_labels = data
            .extra_labels
            .as_ref()
            .map(parse_key_value_pairs)
            .unwrap_or_default();
        labels.append_extra_labels(&extra_labels);
        let labels = labels.get();
        debug!("Parsed labels: {:?}", labels);

        let annotations = Annotations::new_agent_type_id_annotation(&agent_identity.agent_type_id);
        let annotations = annotations.get();

        let helm_repository_obj_meta_data = obj_meta_data(
            REPOSITORY_NAME,
            namespace,
            labels.clone(),
            annotations.clone(),
        );
        // This is not strictly necessary, but it helps to ensure that the labels are consistent
        let mut helm_release_labels = labels;
        helm_release_labels.insert(AGENT_CONTROL_VERSION_SET_FROM.to_string(), source);

        let helm_release_obj_meta_data =
            obj_meta_data(release_name, namespace, helm_release_labels, annotations);

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
