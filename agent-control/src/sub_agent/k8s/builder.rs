use crate::agent_control::config::{AgentID, K8sConfig, SubAgentConfig};
use crate::agent_control::defaults::{CLUSTER_NAME_ATTRIBUTE_KEY, OPAMP_SERVICE_VERSION};
use crate::agent_type::environment::Environment;
use crate::event::channel::{pub_sub, EventPublisher};
use crate::event::SubAgentEvent;
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::opamp::effective_config::loader::EffectiveConfigLoader;
use crate::opamp::instance_id::getter::InstanceIDGetter;
use crate::opamp::operations::build_sub_agent_opamp;
use crate::opamp::remote_config::status_manager::ConfigStatusManager;
use crate::opamp::remote_config::validators::RemoteConfigValidator;
use crate::sub_agent::effective_agents_assembler::{EffectiveAgent, EffectiveAgentsAssembler};
use crate::sub_agent::event_handler::opamp::remote_config_handler::RemoteConfigHandler;
use crate::sub_agent::supervisor::assembler::SupervisorAssembler;
use crate::sub_agent::supervisor::builder::SupervisorBuilder;
use crate::sub_agent::SubAgent;
use crate::sub_agent::SubAgentCallbacks;
use crate::{
    opamp::client_builder::OpAMPClientBuilder,
    sub_agent::k8s::supervisor::NotStartedSupervisorK8s,
    sub_agent::{error::SubAgentBuilderError, SubAgentBuilder},
};
use opamp_client::operation::settings::DescriptionValueType;
use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;
use std::sync::Arc;
use tracing::debug;

pub struct K8sSubAgentBuilder<'a, O, I, A, G, S, M>
where
    G: EffectiveConfigLoader,
    O: OpAMPClientBuilder<SubAgentCallbacks<G>>,
    I: InstanceIDGetter,
    A: EffectiveAgentsAssembler,
    S: RemoteConfigValidator,
    M: ConfigStatusManager,
{
    opamp_builder: Option<&'a O>,
    instance_id_getter: &'a I,
    k8s_client: Arc<SyncK8sClient>,
    effective_agent_assembler: Arc<A>,
    k8s_config: K8sConfig,
    signature_validator: Arc<S>,
    config_status_manager: Arc<M>,

    // This is needed to ensure the generic type parameter G is used in the struct.
    // Else Rust will reject this, complaining that the type parameter is not used.
    _effective_config_loader: PhantomData<G>,
}

impl<'a, O, I, A, G, S, M> K8sSubAgentBuilder<'a, O, I, A, G, S, M>
where
    G: EffectiveConfigLoader,
    O: OpAMPClientBuilder<SubAgentCallbacks<G>>,
    I: InstanceIDGetter,
    A: EffectiveAgentsAssembler,
    S: RemoteConfigValidator,
    M: ConfigStatusManager,
{
    // TODO refactor this new function
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        opamp_builder: Option<&'a O>,
        instance_id_getter: &'a I,
        k8s_client: Arc<SyncK8sClient>,
        effective_agent_assembler: Arc<A>,
        k8s_config: K8sConfig,
        signature_validator: Arc<S>,
        config_status_manager: Arc<M>,
    ) -> Self {
        Self {
            opamp_builder,
            instance_id_getter,
            k8s_client,
            effective_agent_assembler,
            k8s_config,
            signature_validator,
            config_status_manager,

            _effective_config_loader: PhantomData,
        }
    }
}

impl<O, I, A, G, S, M> SubAgentBuilder for K8sSubAgentBuilder<'_, O, I, A, G, S, M>
where
    G: EffectiveConfigLoader + Send + Sync + 'static,
    O: OpAMPClientBuilder<SubAgentCallbacks<G>> + Send + Sync + 'static,
    I: InstanceIDGetter,
    A: EffectiveAgentsAssembler + Send + Sync + 'static,
    S: RemoteConfigValidator + Send + Sync + 'static,
    M: ConfigStatusManager + Send + Sync + 'static,
{
    type NotStartedSubAgent =
        SubAgent<O::Client, SubAgentCallbacks<G>, A, SupervisorBuilderK8s, S, M>;

    fn build(
        &self,
        agent_id: AgentID,
        sub_agent_config: &SubAgentConfig,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
    ) -> Result<Self::NotStartedSubAgent, SubAgentBuilderError> {
        debug!(agent_id = agent_id.to_string(), "building subAgent");

        let (maybe_opamp_client, sub_agent_opamp_consumer) = self
            .opamp_builder
            .map(|builder| {
                build_sub_agent_opamp(
                    builder,
                    self.instance_id_getter,
                    agent_id.clone(),
                    &sub_agent_config.agent_type,
                    HashMap::from([(
                        OPAMP_SERVICE_VERSION.to_string(),
                        sub_agent_config.agent_type.version().into(),
                    )]),
                    HashMap::from([(
                        CLUSTER_NAME_ATTRIBUTE_KEY.to_string(),
                        DescriptionValueType::String(self.k8s_config.cluster_name.to_string()),
                    )]),
                )
            })
            // Transpose changes Option<Result<T, E>> to Result<Option<T>, E>, enabling the use of `?` to handle errors in this function
            .transpose()?
            .map(|(client, consumer)| (Some(client), Some(consumer)))
            .unwrap_or_default();

        let supervisor_builder =
            SupervisorBuilderK8s::new(self.k8s_client.clone(), self.k8s_config.clone());

        let remote_config_handler = RemoteConfigHandler::new(
            agent_id.clone(),
            sub_agent_config.clone(),
            self.signature_validator.clone(),
            self.config_status_manager.clone(),
        );

        let supervisor_assembler = SupervisorAssembler::new(
            self.config_status_manager.clone(),
            supervisor_builder,
            agent_id.clone(),
            sub_agent_config.clone(),
            self.effective_agent_assembler.clone(),
            Environment::K8s,
        );

        Ok(SubAgent::new(
            agent_id,
            sub_agent_config.clone(),
            maybe_opamp_client,
            supervisor_assembler,
            sub_agent_publisher,
            sub_agent_opamp_consumer,
            pub_sub(),
            remote_config_handler,
            self.config_status_manager.clone(),
        ))
    }
}

pub struct SupervisorBuilderK8s {
    k8s_client: Arc<SyncK8sClient>,
    k8s_config: K8sConfig,
}

impl SupervisorBuilderK8s {
    pub fn new(k8s_client: Arc<SyncK8sClient>, k8s_config: K8sConfig) -> Self {
        Self {
            k8s_client,
            k8s_config,
        }
    }
}

impl SupervisorBuilder for SupervisorBuilderK8s {
    type SupervisorStarter = NotStartedSupervisorK8s;

    fn build_supervisor(
        &self,
        effective_agent: EffectiveAgent,
    ) -> Result<Self::SupervisorStarter, SubAgentBuilderError> {
        let agent_id = effective_agent.get_agent_id().clone();
        let agent_type = effective_agent.get_agent_type().clone();
        debug!("Building supervisors {}:{}", agent_type, agent_id);

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
            agent_id,
            agent_type,
            self.k8s_client.clone(),
            k8s_objects.clone(),
        ))
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::agent_control::config::AgentTypeFQN;
    use crate::agent_control::defaults::PARENT_AGENT_ID_ATTRIBUTE_KEY;
    use crate::agent_type::runtime_config::{self, Deployment, K8sObject, Runtime};
    use crate::event::channel::pub_sub;
    use crate::opamp::client_builder::tests::MockStartedOpAMPClientMock;
    use crate::opamp::client_builder::OpAMPClientBuilderError;
    use crate::opamp::effective_config::loader::tests::MockEffectiveConfigLoaderMock;
    use crate::opamp::http::builder::HttpClientBuilderError;
    use crate::opamp::instance_id::getter::tests::MockInstanceIDGetterMock;
    use crate::opamp::instance_id::InstanceID;
    use crate::opamp::operations::start_settings;
    use crate::opamp::remote_config::status_manager::tests::MockConfigStatusManagerMock;
    use crate::opamp::remote_config::validators::tests::MockRemoteConfigValidatorMock;
    use crate::sub_agent::effective_agents_assembler::tests::MockEffectiveAgentAssemblerMock;
    use crate::{
        k8s::client::MockSyncK8sClient, opamp::client_builder::tests::MockOpAMPClientBuilderMock,
    };
    use assert_matches::assert_matches;
    use mockall::predicate;
    use opamp_client::operation::settings::DescriptionValueType;
    use std::collections::HashMap;

    const TEST_CLUSTER_NAME: &str = "cluster_name";
    const TEST_NAMESPACE: &str = "test-namespace";
    const TEST_AGENT_ID: &str = "k8s-test";

    #[test]
    fn k8s_agent_build_success() {
        let agent_id = AgentID::new(TEST_AGENT_ID).unwrap();
        let sub_agent_config = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("newrelic/com.newrelic.infrastructure:0.0.2")
                .unwrap(),
        };

        // instance K8s client mock
        let mock_client = MockSyncK8sClient::default();

        let (opamp_builder, instance_id_getter) =
            k8s_agent_get_common_mocks(sub_agent_config.clone(), agent_id.clone(), false);

        let k8s_config = K8sConfig {
            cluster_name: TEST_CLUSTER_NAME.to_string(),
            namespace: TEST_NAMESPACE.to_string(),
            cr_type_meta: K8sConfig::default().cr_type_meta,
            ..Default::default()
        };

        let assembler = MockEffectiveAgentAssemblerMock::new();
        let config_status_manager = MockConfigStatusManagerMock::default();

        let signature_validator = MockRemoteConfigValidatorMock::new();

        let builder = K8sSubAgentBuilder::new(
            Some(&opamp_builder),
            &instance_id_getter,
            Arc::new(mock_client),
            Arc::new(assembler),
            k8s_config,
            Arc::new(signature_validator),
            Arc::new(config_status_manager),
        );

        let (application_event_publisher, _) = pub_sub();
        builder
            .build(agent_id, &sub_agent_config, application_event_publisher)
            .unwrap();
    }

    #[test]
    fn k8s_agent_error_building_opamp() {
        let agent_id = AgentID::new(TEST_AGENT_ID).unwrap();
        let sub_agent_config = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("newrelic/com.newrelic.infrastructure:0.0.2")
                .unwrap(),
        };

        // instance K8s client mock
        let mock_client = MockSyncK8sClient::default();

        let (opamp_builder, instance_id_getter) =
            k8s_agent_get_common_mocks(sub_agent_config.clone(), agent_id.clone(), true);

        let k8s_config = K8sConfig {
            cluster_name: TEST_CLUSTER_NAME.to_string(),
            namespace: TEST_NAMESPACE.to_string(),
            cr_type_meta: K8sConfig::default().cr_type_meta,
            ..Default::default()
        };

        let assembler = MockEffectiveAgentAssemblerMock::new();
        let config_status_manager = MockConfigStatusManagerMock::default();

        let signature_validator = MockRemoteConfigValidatorMock::new();

        let builder = K8sSubAgentBuilder::new(
            Some(&opamp_builder),
            &instance_id_getter,
            Arc::new(mock_client),
            Arc::new(assembler),
            k8s_config,
            Arc::new(signature_validator),
            Arc::new(config_status_manager),
        );

        let (application_event_publisher, _) = pub_sub();
        let result = builder.build(agent_id, &sub_agent_config, application_event_publisher);
        assert_matches!(
            result.err().expect("Expected error"),
            SubAgentBuilderError::OpampClientBuilderError(_)
        );
    }

    #[test]
    fn supervisor_build_ok() {
        let agent_id = AgentID::new(TEST_AGENT_ID).unwrap();
        let sub_agent_config = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("newrelic/com.newrelic.infrastructure:0.0.2")
                .unwrap(),
        };

        let effective_agent = EffectiveAgent::new(
            agent_id,
            sub_agent_config.agent_type,
            Runtime {
                deployment: Deployment {
                    on_host: None,
                    k8s: Some(k8s_sample_runtime_config(true)),
                },
            },
        );

        let supervisor_builder = testing_supervisor_builder();

        let result = supervisor_builder.build_supervisor(effective_agent);
        assert!(
            result.is_ok(),
            "It should not error and it should return a supervisor"
        );
    }

    #[test]
    fn supervisor_build_fails_for_invalid_k8s_object_kind() {
        let agent_id = AgentID::new(TEST_AGENT_ID).unwrap();
        let sub_agent_config = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("newrelic/com.newrelic.infrastructure:0.0.2")
                .unwrap(),
        };

        let effective_agent = EffectiveAgent::new(
            agent_id,
            sub_agent_config.agent_type,
            Runtime {
                deployment: Deployment {
                    on_host: None,
                    k8s: Some(k8s_sample_runtime_config(false)),
                },
            },
        );

        let supervisor_builder = testing_supervisor_builder();

        let result = supervisor_builder.build_supervisor(effective_agent);
        assert_matches!(
            result.err().expect("Expected error"),
            SubAgentBuilderError::UnsupportedK8sObject(_)
        );
    }

    pub fn k8s_sample_runtime_config(valid_kind: bool) -> runtime_config::K8s {
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
        runtime_config::K8s {
            objects,
            health: None,
        }
    }

    fn k8s_agent_get_common_mocks(
        sub_agent_config: SubAgentConfig,
        agent_id: AgentID,
        opamp_builder_fails: bool,
    ) -> (
        MockOpAMPClientBuilderMock<SubAgentCallbacks<MockEffectiveConfigLoaderMock>>,
        MockInstanceIDGetterMock,
    ) {
        let instance_id = InstanceID::try_from("018FCA0670A879689D04fABDDE189B8C").unwrap();

        // opamp builder mock
        let started_client = MockStartedOpAMPClientMock::new();
        let mut opamp_builder = MockOpAMPClientBuilderMock::new();
        let start_settings = start_settings(
            instance_id.clone(),
            &sub_agent_config.agent_type,
            HashMap::from([(
                OPAMP_SERVICE_VERSION.to_string(),
                sub_agent_config.agent_type.version().into(),
            )]),
            HashMap::from([
                (
                    CLUSTER_NAME_ATTRIBUTE_KEY.to_string(),
                    DescriptionValueType::String(TEST_CLUSTER_NAME.to_string()),
                ),
                (
                    PARENT_AGENT_ID_ATTRIBUTE_KEY.to_string(),
                    DescriptionValueType::Bytes(instance_id.clone().into()),
                ),
            ]),
        );
        if opamp_builder_fails {
            opamp_builder
                .expect_build_and_start()
                .with(
                    predicate::always(),
                    predicate::eq(agent_id.clone()),
                    predicate::eq(start_settings),
                )
                .once()
                .return_once(move |_, _, _| {
                    Err(OpAMPClientBuilderError::HttpClientBuilderError(
                        HttpClientBuilderError::BuildingError("error".into()),
                    ))
                });
        } else {
            opamp_builder.should_build_and_start(agent_id.clone(), start_settings, started_client);
        }

        // instance id getter mock
        let mut instance_id_getter = MockInstanceIDGetterMock::new();
        instance_id_getter.should_get(&agent_id, instance_id.clone());
        instance_id_getter.should_get(&AgentID::new_agent_control_id(), instance_id);

        (opamp_builder, instance_id_getter)
    }

    fn testing_supervisor_builder() -> SupervisorBuilderK8s {
        let mut mock_client = MockSyncK8sClient::default();
        mock_client
            .expect_default_namespace()
            .return_const("default".to_string());

        let k8s_config = K8sConfig {
            cluster_name: TEST_CLUSTER_NAME.to_string(),
            namespace: TEST_NAMESPACE.to_string(),
            cr_type_meta: K8sConfig::default().cr_type_meta,
            ..Default::default()
        };
        SupervisorBuilderK8s::new(Arc::new(mock_client), k8s_config)
    }
}
