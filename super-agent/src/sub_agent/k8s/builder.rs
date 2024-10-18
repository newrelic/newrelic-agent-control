use super::sub_agent::SubAgentK8s;
use super::supervisor::StartedSupervisor;
use crate::agent_type::runtime_config::K8sObject;
use crate::event::channel::{pub_sub, EventPublisher};
use crate::event::SubAgentEvent;
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::opamp::effective_config::loader::EffectiveConfigLoader;
use crate::opamp::hash_repository::HashRepository;
use crate::opamp::instance_id::getter::InstanceIDGetter;
use crate::opamp::operations::build_sub_agent_opamp;
use crate::sub_agent::build_supervisor_or_default;
use crate::sub_agent::effective_agents_assembler::{
    EffectiveAgent, EffectiveAgentsAssembler, EffectiveAgentsAssemblerError,
};
use crate::sub_agent::event_processor_builder::SubAgentEventProcessorBuilder;
use crate::sub_agent::supervisor::SupervisorBuilder;
use crate::sub_agent::{NotStarted, SubAgentCallbacks};
use crate::super_agent::config::{AgentID, K8sConfig, SubAgentConfig};
use crate::super_agent::defaults::CLUSTER_NAME_ATTRIBUTE_KEY;
use crate::{
    opamp::client_builder::OpAMPClientBuilder,
    sub_agent::k8s::supervisor::NotStartedSupervisor,
    sub_agent::{error::SubAgentBuilderError, SubAgentBuilder},
};
use kube::core::TypeMeta;
use opamp_client::operation::settings::DescriptionValueType;
use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;
use std::sync::Arc;
use tracing::debug;

pub struct K8sSubAgentBuilder<'a, O, I, HR, A, E, G>
where
    G: EffectiveConfigLoader,
    O: OpAMPClientBuilder<SubAgentCallbacks<G>>,
    I: InstanceIDGetter,
    HR: HashRepository,
    A: EffectiveAgentsAssembler,
    E: SubAgentEventProcessorBuilder<O::Client, G>,
{
    opamp_builder: Option<&'a O>,
    instance_id_getter: &'a I,
    hash_repository: Arc<HR>,
    k8s_client: Arc<SyncK8sClient>,
    effective_agent_assembler: &'a A,
    event_processor_builder: &'a E,
    k8s_config: K8sConfig,

    // This is needed to ensure the generic type parameter G is used in the struct.
    // Else Rust will reject this, complaining that the type parameter is not used.
    _effective_config_loader: PhantomData<G>,
}

impl<'a, O, I, HR, A, E, G> K8sSubAgentBuilder<'a, O, I, HR, A, E, G>
where
    G: EffectiveConfigLoader,
    O: OpAMPClientBuilder<SubAgentCallbacks<G>>,
    I: InstanceIDGetter,
    HR: HashRepository,
    A: EffectiveAgentsAssembler,
    E: SubAgentEventProcessorBuilder<O::Client, G>,
{
    pub fn new(
        opamp_builder: Option<&'a O>,
        instance_id_getter: &'a I,
        k8s_client: Arc<SyncK8sClient>,
        hash_repository: Arc<HR>,
        effective_agent_assembler: &'a A,
        event_processor_builder: &'a E,
        k8s_config: K8sConfig,
    ) -> Self {
        Self {
            opamp_builder,
            instance_id_getter,
            hash_repository,
            k8s_client,
            effective_agent_assembler,
            event_processor_builder,
            k8s_config,

            _effective_config_loader: PhantomData,
        }
    }
}

impl<'a, O, I, HR, A, E, G> SubAgentBuilder for K8sSubAgentBuilder<'a, O, I, HR, A, E, G>
where
    G: EffectiveConfigLoader,
    O: OpAMPClientBuilder<SubAgentCallbacks<G>>,
    I: InstanceIDGetter,
    HR: HashRepository,
    A: EffectiveAgentsAssembler,
    E: SubAgentEventProcessorBuilder<O::Client, G>,
{
    type NotStartedSubAgent = SubAgentK8s<
        'a,
        NotStarted<E::SubAgentEventProcessor>,
        StartedSupervisor,
        O::Client,
        SubAgentCallbacks<G>,
        A,
        SupervisorBuilderK8s<O, HR, G>,
    >;

    fn build(
        &self,
        agent_id: AgentID,
        sub_agent_config: &SubAgentConfig,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
    ) -> Result<Self::NotStartedSubAgent, SubAgentBuilderError> {
        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();

        debug!(agent_id = agent_id.to_string(), "building subAgent");

        let agent_fqn = sub_agent_config.agent_type.clone();

        let (maybe_opamp_client, sub_agent_opamp_consumer) = self
            .opamp_builder
            .map(|builder| {
                build_sub_agent_opamp(
                    builder,
                    self.instance_id_getter,
                    agent_id.clone(),
                    &sub_agent_config.agent_type,
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

        let maybe_opamp_client = Arc::new(maybe_opamp_client);

        let supervisor_builder = SupervisorBuilderK8s::new(
            agent_id.clone(),
            sub_agent_config.clone(),
            self.hash_repository.clone(),
            self.k8s_client.clone(),
            self.k8s_config.clone(),
        );

        let event_processor = self.event_processor_builder.build(
            agent_id.clone(),
            agent_fqn,
            sub_agent_publisher,
            sub_agent_opamp_consumer,
            sub_agent_internal_consumer,
            maybe_opamp_client.clone(),
        );

        Ok(SubAgentK8s::new(
            agent_id,
            sub_agent_config.clone(),
            event_processor,
            sub_agent_internal_publisher,
            maybe_opamp_client,
            self.effective_agent_assembler,
            supervisor_builder,
        ))
    }
}

pub struct SupervisorBuilderK8s<O, HR, G>
where
    G: EffectiveConfigLoader,
    O: OpAMPClientBuilder<SubAgentCallbacks<G>>,
    HR: HashRepository,
{
    agent_id: AgentID,
    agent_cfg: SubAgentConfig,
    hash_repository: Arc<HR>,
    k8s_client: Arc<SyncK8sClient>,
    k8s_config: K8sConfig,

    // This is needed to ensure the generic type parameters O and G are used.
    // Else Rust will reject this, complaining that the type parameter is not used.
    _opamp_client_builder: PhantomData<O>,
    _effective_config_loader: PhantomData<G>,
}

impl<O, HR, G> SupervisorBuilderK8s<O, HR, G>
where
    G: EffectiveConfigLoader,
    O: OpAMPClientBuilder<SubAgentCallbacks<G>>,
    HR: HashRepository,
{
    pub fn new(
        agent_id: AgentID,
        agent_cfg: SubAgentConfig,
        hash_repository: Arc<HR>,
        k8s_client: Arc<SyncK8sClient>,
        k8s_config: K8sConfig,
    ) -> Self {
        Self {
            agent_id,
            agent_cfg,
            hash_repository,
            k8s_client,
            k8s_config,
            _opamp_client_builder: PhantomData,
            _effective_config_loader: PhantomData,
        }
    }

    pub fn build_cr_supervisors(
        &self,
        effective_agent: EffectiveAgent,
    ) -> Result<NotStartedSupervisor, SubAgentBuilderError> {
        debug!("Building CR supervisors {}", &self.agent_id);

        let k8s_objects = effective_agent.get_k8s_config()?;

        // Validate Kubernetes objects against the list of supported resources.
        Self::validate_k8s_objects(&k8s_objects.objects.clone(), &self.k8s_config.cr_type_meta)?;

        // Clone the k8s_client on each build.
        Ok(NotStartedSupervisor::new(
            self.agent_id.clone(),
            self.agent_cfg.agent_type.clone(),
            self.k8s_client.clone(),
            k8s_objects.clone(),
        ))
    }

    fn validate_k8s_objects(
        objects: &HashMap<String, K8sObject>,
        supported_types: &[TypeMeta],
    ) -> Result<(), SubAgentBuilderError> {
        let supported_set: HashSet<(&str, &str)> = supported_types
            .iter()
            .map(|tm| (tm.api_version.as_str(), tm.kind.as_str()))
            .collect();

        for k8s_obj in objects.values() {
            let obj_key = (k8s_obj.api_version.as_str(), k8s_obj.kind.as_str());
            if !supported_set.contains(&obj_key) {
                return Err(SubAgentBuilderError::UnsupportedK8sObject(format!(
                    "Unsupported Kubernetes object with api_version '{}' and kind '{}'",
                    k8s_obj.api_version, k8s_obj.kind
                )));
            }
        }
        Ok(())
    }
}

impl<O, HR, G> SupervisorBuilder for SupervisorBuilderK8s<O, HR, G>
where
    G: EffectiveConfigLoader,
    O: OpAMPClientBuilder<SubAgentCallbacks<G>>,
    HR: HashRepository,
{
    type Supervisor = NotStartedSupervisor;

    type OpAMPClient = O::Client;

    fn build_supervisor(
        &self,
        effective_agent_result: Result<EffectiveAgent, EffectiveAgentsAssemblerError>,
        maybe_opamp_client: &Option<Self::OpAMPClient>,
    ) -> Result<Option<Self::Supervisor>, SubAgentBuilderError> {
        build_supervisor_or_default::<HR, O, _, _, _>(
            &self.agent_id,
            &self.hash_repository,
            maybe_opamp_client,
            effective_agent_result,
            |effective_agent| self.build_cr_supervisors(effective_agent).map(Some),
        )
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::agent_type::runtime_config::{self, Deployment, Runtime};
    use crate::event::channel::pub_sub;
    use crate::opamp::client_builder::test::MockStartedOpAMPClientMock;
    use crate::opamp::client_builder::OpAMPClientBuilderError;
    use crate::opamp::effective_config::loader::tests::MockEffectiveConfigLoaderMock;
    use crate::opamp::hash_repository::repository::test::MockHashRepositoryMock;
    use crate::opamp::http::builder::HttpClientBuilderError;
    use crate::opamp::instance_id::getter::test::MockInstanceIDGetterMock;
    use crate::opamp::instance_id::InstanceID;
    use crate::opamp::operations::start_settings;
    use crate::sub_agent::effective_agents_assembler::tests::MockEffectiveAgentAssemblerMock;
    use crate::sub_agent::event_processor::test::MockEventProcessorMock;
    use crate::sub_agent::event_processor_builder::test::MockSubAgentEventProcessorBuilderMock;
    use crate::sub_agent::k8s::sub_agent::test::TEST_AGENT_ID;
    use crate::super_agent::config::AgentTypeFQN;
    use crate::super_agent::defaults::PARENT_AGENT_ID_ATTRIBUTE_KEY;
    use crate::{
        k8s::client::MockSyncK8sClient, opamp::client_builder::test::MockOpAMPClientBuilderMock,
    };
    use assert_matches::assert_matches;
    use mockall::predicate;
    use opamp_client::operation::settings::DescriptionValueType;
    use std::collections::HashMap;

    const TEST_CLUSTER_NAME: &str = "cluster_name";
    const TEST_NAMESPACE: &str = "test-namespace";
    #[test]
    fn k8s_agent_build_success() {
        let agent_id = AgentID::new(TEST_AGENT_ID).unwrap();
        let sub_agent_config = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("newrelic/com.newrelic.infrastructure_agent:0.0.2")
                .unwrap(),
        };

        // instance K8s client mock
        let mock_client = MockSyncK8sClient::default();

        // event processor mock
        let mut sub_agent_event_processor_builder = MockSubAgentEventProcessorBuilderMock::new();
        sub_agent_event_processor_builder.should_build(MockEventProcessorMock::default());

        let (opamp_builder, instance_id_getter, hash_repository_mock) =
            k8s_agent_get_common_mocks(sub_agent_config.clone(), agent_id.clone(), false);

        let k8s_config = K8sConfig {
            cluster_name: TEST_CLUSTER_NAME.to_string(),
            namespace: TEST_NAMESPACE.to_string(),
            cr_type_meta: K8sConfig::default().cr_type_meta,
        };

        let assembler = MockEffectiveAgentAssemblerMock::new();

        let builder = K8sSubAgentBuilder::new(
            Some(&opamp_builder),
            &instance_id_getter,
            Arc::new(mock_client),
            Arc::new(hash_repository_mock),
            &assembler,
            &sub_agent_event_processor_builder,
            k8s_config,
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
            agent_type: AgentTypeFQN::try_from("newrelic/com.newrelic.infrastructure_agent:0.0.2")
                .unwrap(),
        };

        // instance K8s client mock
        let mock_client = MockSyncK8sClient::default();

        // event processor mock
        let sub_agent_event_processor_builder = MockSubAgentEventProcessorBuilderMock::new();

        let (opamp_builder, instance_id_getter, hash_repository_mock) =
            k8s_agent_get_common_mocks(sub_agent_config.clone(), agent_id.clone(), true);

        let k8s_config = K8sConfig {
            cluster_name: TEST_CLUSTER_NAME.to_string(),
            namespace: TEST_NAMESPACE.to_string(),
            cr_type_meta: K8sConfig::default().cr_type_meta,
        };

        let assembler = MockEffectiveAgentAssemblerMock::new();

        let builder = K8sSubAgentBuilder::new(
            Some(&opamp_builder),
            &instance_id_getter,
            Arc::new(mock_client),
            Arc::new(hash_repository_mock),
            &assembler,
            &sub_agent_event_processor_builder,
            k8s_config,
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
            agent_type: AgentTypeFQN::try_from("newrelic/com.newrelic.infrastructure_agent:0.0.2")
                .unwrap(),
        };

        let effective_agent = EffectiveAgent::new(
            agent_id.clone(),
            sub_agent_config.agent_type.clone(),
            Runtime {
                deployment: Deployment {
                    on_host: None,
                    k8s: Some(k8s_sample_runtime_config(true)),
                },
            },
        );

        let supervisor_builder =
            testing_supervisor_builder(agent_id.clone(), sub_agent_config.clone());

        let result = supervisor_builder.build_supervisor(Ok(effective_agent), &None);
        assert!(
            result.unwrap().is_some(),
            "It should not error and it should return a supervisor"
        );
    }

    #[test]
    fn supervisor_build_fails_for_invalid_k8s_object_kind() {
        let agent_id = AgentID::new(TEST_AGENT_ID).unwrap();
        let sub_agent_config = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("newrelic/com.newrelic.infrastructure_agent:0.0.2")
                .unwrap(),
        };

        let effective_agent = EffectiveAgent::new(
            agent_id.clone(),
            sub_agent_config.agent_type.clone(),
            Runtime {
                deployment: Deployment {
                    on_host: None,
                    k8s: Some(k8s_sample_runtime_config(false)),
                },
            },
        );

        let supervisor_builder =
            testing_supervisor_builder(agent_id.clone(), sub_agent_config.clone());

        let result = supervisor_builder.build_supervisor(Ok(effective_agent), &None);
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
        MockHashRepositoryMock,
    ) {
        let instance_id = InstanceID::try_from("018FCA0670A879689D04fABDDE189B8C").unwrap();

        // opamp builder mock
        let started_client = MockStartedOpAMPClientMock::new();
        let mut opamp_builder = MockOpAMPClientBuilderMock::new();
        let start_settings = start_settings(
            instance_id.clone(),
            &sub_agent_config.agent_type,
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
        instance_id_getter.should_get(&AgentID::new_super_agent_id(), instance_id);

        // hash_repository_mock
        let hash_repository_mock = MockHashRepositoryMock::new();

        (opamp_builder, instance_id_getter, hash_repository_mock)
    }

    fn testing_supervisor_builder(
        agent_id: AgentID,
        sub_agent_config: SubAgentConfig,
    ) -> SupervisorBuilderK8s<
        MockOpAMPClientBuilderMock<SubAgentCallbacks<MockEffectiveConfigLoaderMock>>,
        MockHashRepositoryMock,
        MockEffectiveConfigLoaderMock,
    > {
        let hash_repository = MockHashRepositoryMock::new();

        let mut mock_client = MockSyncK8sClient::default();
        mock_client
            .expect_default_namespace()
            .return_const("default".to_string());

        let k8s_config = K8sConfig {
            cluster_name: TEST_CLUSTER_NAME.to_string(),
            namespace: TEST_NAMESPACE.to_string(),
            cr_type_meta: K8sConfig::default().cr_type_meta,
        };
        SupervisorBuilderK8s::new(
            agent_id,
            sub_agent_config,
            Arc::new(hash_repository),
            Arc::new(mock_client),
            k8s_config,
        )
    }
}
