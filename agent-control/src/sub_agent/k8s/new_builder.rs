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

        Ok(NotStartedSupervisorK8s::new(
            agent_identity.clone(),
            self.k8s_client.clone(),
            k8s_objects.clone(),
        ))
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::agent_control::agent_id::AgentID;
    use crate::agent_control::config::K8sConfig;
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::agent_type::runtime_config::k8s::{K8s, K8sObject};
    use crate::agent_type::runtime_config::rendered::{Deployment, Runtime};
    use crate::k8s::client::MockSyncK8sClient;
    use crate::sub_agent::error::SubAgentBuilderError;
    use crate::sub_agent::identity::AgentIdentity;
    use assert_matches::assert_matches;
    use std::collections::HashMap;
    use std::sync::Arc;

    const TEST_CLUSTER_NAME: &str = "cluster_name";
    const TEST_AGENT_ID: &str = "k8s-test";

    #[test]
    fn supervisor_build_ok() {
        let agent_identity = AgentIdentity::from((
            AgentID::try_from(TEST_AGENT_ID).unwrap(),
            AgentTypeID::try_from("newrelic/com.newrelic.infrastructure:0.0.2").unwrap(),
        ));

        let effective_agent = EffectiveAgent::new(
            agent_identity,
            Runtime {
                deployment: Deployment {
                    linux: None,
                    windows: None,
                    k8s: Some(k8s_sample_runtime_config(true)),
                },
            },
        );

        let supervisor_builder = testing_supervisor_builder();

        supervisor_builder
            .build_supervisor(effective_agent)
            .unwrap();
    }

    #[test]
    fn supervisor_build_fails_for_invalid_k8s_object_kind() {
        let agent_identity = AgentIdentity::from((
            AgentID::try_from(TEST_AGENT_ID).unwrap(),
            AgentTypeID::try_from("newrelic/com.newrelic.infrastructure:0.0.2").unwrap(),
        ));

        let effective_agent = EffectiveAgent::new(
            agent_identity,
            Runtime {
                deployment: Deployment {
                    linux: None,
                    windows: None,
                    k8s: Some(k8s_sample_runtime_config(false)),
                },
            },
        );

        let supervisor_builder = testing_supervisor_builder();

        let result = supervisor_builder.build_supervisor(effective_agent);
        assert_matches!(
            result.expect_err("Expected error"),
            SubAgentBuilderError::UnsupportedK8sObject(_)
        );
    }

    pub fn k8s_sample_runtime_config(valid_kind: bool) -> K8s {
        let kind = if valid_kind {
            "HelmRelease".to_string()
        } else {
            "UnsupportedKind".to_string()
        };

        let k8s_object = K8sObject {
            api_version: "helm.toolkit.fluxcd.io/v2".to_string(),
            kind,
            ..Default::default()
        };

        let mut objects = HashMap::new();
        objects.insert("sample_object".to_string(), k8s_object);
        K8s {
            objects,
            ..K8s::default()
        }
    }

    fn testing_supervisor_builder() -> SupervisorBuilderK8s {
        let mock_client = MockSyncK8sClient::default();

        let k8s_config = K8sConfig {
            cluster_name: TEST_CLUSTER_NAME.to_string(),
            cr_type_meta: K8sConfig::default().cr_type_meta,
            ..Default::default()
        };
        SupervisorBuilderK8s::new(Arc::new(mock_client), k8s_config)
    }
}
