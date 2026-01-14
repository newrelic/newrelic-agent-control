use std::collections::HashSet;

use tracing::debug;

use crate::sub_agent::{
    effective_agents_assembler::EffectiveAgent,
    error::SubAgentBuilderError,
    k8s::{builder::SupervisorBuilderK8s, supervisor::NotStartedSupervisorK8s},
    supervisor::SupervisorBuilder,
};

impl SupervisorBuilder for SupervisorBuilderK8s {
    type Starter = NotStartedSupervisorK8s;
    type Error = SubAgentBuilderError;

    fn build_supervisor(
        &self,
        effective_agent: EffectiveAgent,
    ) -> Result<Self::Starter, Self::Error> {
        let agent_identity = effective_agent.get_agent_identity();
        debug!("Building supervisors {}", agent_identity,);

        let k8s_objects = effective_agent.get_k8s_config()?;

        // Validate Kubernetes objects against the list of supported resources.
        let supported_set: HashSet<(&str, &str)> = self
            .k8s_config
            .cr_type_meta
            .iter()
            .map(|tm| (tm.api_version.as_str(), tm.kind.as_str()))
            .collect();

        for k8s_obj in k8s_objects.objects.values() {
            let obj_key = (k8s_obj.api_version.as_str(), k8s_obj.kind.as_str());
            if !supported_set.contains(&obj_key) {
                return Err(SubAgentBuilderError::UnsupportedK8sObject(format!(
                    "Unsupported Kubernetes object with api_version '{}' and kind '{}'",
                    k8s_obj.api_version, k8s_obj.kind
                )));
            }
        }

        // Clone the k8s_client on each build.
        Ok(NotStartedSupervisorK8s::new(
            agent_identity.clone(),
            self.k8s_client.clone(),
            k8s_objects.clone(),
        ))
    }
}
