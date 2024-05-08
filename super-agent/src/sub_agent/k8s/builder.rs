use super::sub_agent::SubAgentK8s;
use crate::agent_type::environment::Environment;
use crate::agent_type::runtime_config::K8sObject;
use crate::event::channel::{pub_sub, EventPublisher};
use crate::event::SubAgentEvent;
#[cfg_attr(test, mockall_double::double)]
use crate::k8s::client::SyncK8sClient;
use crate::opamp::hash_repository::HashRepository;
use crate::opamp::instance_id::getter::InstanceIDGetter;
use crate::opamp::operations::build_sub_agent_opamp;
use crate::sub_agent::build_supervisor_from_effective_agent;
use crate::sub_agent::effective_agents_assembler::{EffectiveAgent, EffectiveAgentsAssembler};
use crate::sub_agent::event_processor_builder::SubAgentEventProcessorBuilder;
use crate::sub_agent::{NotStarted, SubAgentCallbacks};
use crate::super_agent::config::{AgentID, K8sConfig, SubAgentConfig};
use crate::super_agent::defaults::CLUSTER_NAME_ATTRIBUTE_KEY;
use crate::{
    opamp::client_builder::OpAMPClientBuilder,
    sub_agent::k8s::supervisor::CRSupervisor,
    sub_agent::{error::SubAgentBuilderError, SubAgentBuilder},
};
use kube::core::TypeMeta;
use opamp_client::operation::settings::DescriptionValueType;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tracing::debug;

pub struct K8sSubAgentBuilder<'a, O, I, HR, A, E>
where
    O: OpAMPClientBuilder<SubAgentCallbacks>,
    I: InstanceIDGetter,
    HR: HashRepository,
    A: EffectiveAgentsAssembler,
    E: SubAgentEventProcessorBuilder<O::Client>,
{
    opamp_builder: Option<&'a O>,
    instance_id_getter: &'a I,
    hash_repository: Arc<HR>,
    k8s_client: Arc<SyncK8sClient>,
    effective_agent_assembler: &'a A,
    event_processor_builder: &'a E,
    k8s_config: K8sConfig,
}

impl<'a, O, I, HR, A, E> K8sSubAgentBuilder<'a, O, I, HR, A, E>
where
    O: OpAMPClientBuilder<SubAgentCallbacks>,
    I: InstanceIDGetter,
    HR: HashRepository,
    A: EffectiveAgentsAssembler,
    E: SubAgentEventProcessorBuilder<O::Client>,
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
        }
    }
}

impl<'a, O, I, HR, A, E> SubAgentBuilder for K8sSubAgentBuilder<'a, O, I, HR, A, E>
where
    O: OpAMPClientBuilder<SubAgentCallbacks>,
    I: InstanceIDGetter,
    HR: HashRepository,
    A: EffectiveAgentsAssembler,
    E: SubAgentEventProcessorBuilder<O::Client>,
{
    type NotStartedSubAgent = SubAgentK8s<NotStarted<E::SubAgentEventProcessor>>;

    fn build(
        &self,
        agent_id: AgentID,
        sub_agent_config: &SubAgentConfig,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
    ) -> Result<Self::NotStartedSubAgent, SubAgentBuilderError> {
        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();

        debug!(agent_id = agent_id.to_string(), "building subAgent");

        let effective_agent_res = self.effective_agent_assembler.assemble_agent(
            &agent_id,
            sub_agent_config,
            &Environment::K8s,
        );

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

        // A sub-agent can be started without supervisor, when running with opamp activated, in order to
        // be able to receive messages.
        let supervisor = build_supervisor_from_effective_agent::<HR, O, _, _>(
            &agent_id,
            &self.hash_repository,
            &maybe_opamp_client,
            effective_agent_res,
            |effective_agent| {
                build_cr_supervisors(
                    &agent_id,
                    effective_agent,
                    self.k8s_client.clone(),
                    &self.k8s_config,
                )
                .map(Some) // Doing this as `supervisor` is expected to be an Option<_>.
                           // It also ensures the return type has a Default (None) so it complies with the signature for `build_supervisor_from_effective_agent`
            },
        )?;

        let event_processor = self.event_processor_builder.build(
            agent_id.clone(),
            sub_agent_publisher,
            sub_agent_opamp_consumer,
            sub_agent_internal_consumer,
            maybe_opamp_client,
        );

        Ok(SubAgentK8s::new(
            agent_id,
            sub_agent_config.agent_type.clone(),
            event_processor,
            sub_agent_internal_publisher,
            supervisor,
        ))
    }
}

fn build_cr_supervisors(
    agent_id: &AgentID,
    effective_agent: EffectiveAgent,
    k8s_client: Arc<SyncK8sClient>,
    k8s_config: &K8sConfig,
) -> Result<CRSupervisor, SubAgentBuilderError> {
    debug!("Building CR supervisors {}", agent_id);

    let k8s_objects = effective_agent
        .get_runtime_config()
        .deployment
        .k8s
        .as_ref()
        .ok_or(SubAgentBuilderError::ConfigError(
            "Missing k8s deployment configuration".into(),
        ))?;

    // Validate Kubernetes objects against the list of supported resources.
    validate_k8s_objects(&k8s_objects.objects.clone(), &k8s_config.cr_type_meta)?;

    // Clone the k8s_client on each build.
    Ok(CRSupervisor::new(
        agent_id.clone(),
        k8s_client,
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

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::agent_type::runtime_config;
    use crate::agent_type::runtime_config::{Deployment, Runtime};
    use crate::event::channel::pub_sub;
    use crate::opamp::client_builder::test::MockStartedOpAMPClientMock;
    use crate::opamp::hash_repository::repository::test::MockHashRepositoryMock;
    use crate::opamp::instance_id::getter::test::MockInstanceIDGetterMock;
    use crate::opamp::operations::start_settings;
    use crate::opamp::remote_config_hash::Hash;
    use crate::sub_agent::effective_agents_assembler::tests::MockEffectiveAgentAssemblerMock;
    use crate::sub_agent::effective_agents_assembler::EffectiveAgent;
    use crate::sub_agent::event_processor::test::MockEventProcessorMock;
    use crate::sub_agent::event_processor_builder::test::MockSubAgentEventProcessorBuilderMock;
    use crate::sub_agent::k8s::sub_agent::test::TEST_AGENT_ID;
    use crate::super_agent::config::AgentTypeFQN;
    use crate::super_agent::defaults::PARENT_AGENT_ID_ATTRIBUTE_KEY;
    use crate::{
        k8s::client::MockSyncK8sClient, opamp::client_builder::test::MockOpAMPClientBuilderMock,
    };
    use assert_matches::assert_matches;
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
        let mut mock_client = MockSyncK8sClient::default();
        mock_client
            .expect_default_namespace()
            .return_const("default".to_string());

        // event processor mock
        let mut sub_agent_event_processor_builder = MockSubAgentEventProcessorBuilderMock::new();
        sub_agent_event_processor_builder.should_build(MockEventProcessorMock::default());

        let (opamp_builder, instance_id_getter, hash_repository_mock) =
            get_common_mocks(sub_agent_config.clone(), agent_id.clone());

        let k8s_config = K8sConfig {
            cluster_name: TEST_CLUSTER_NAME.to_string(),
            namespace: TEST_NAMESPACE.to_string(),
            cr_type_meta: K8sConfig::default().cr_type_meta,
        };

        let assembler = get_agent_assembler_mock(sub_agent_config.clone(), agent_id.clone(), true);
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
    fn build_error_with_invalid_object_kind() {
        let agent_id = AgentID::new(TEST_AGENT_ID).unwrap();
        let sub_agent_config = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("newrelic/com.newrelic.infrastructure_agent:0.0.2")
                .unwrap(),
        };

        // event processor mock
        let mut sub_agent_event_processor_builder = MockSubAgentEventProcessorBuilderMock::new();
        sub_agent_event_processor_builder.expect_build().never();

        let (opamp_builder, instance_id_getter, hash_repository_mock) =
            get_common_mocks(sub_agent_config.clone(), agent_id.clone());

        let k8s_config = K8sConfig {
            cluster_name: TEST_CLUSTER_NAME.to_string(),
            namespace: TEST_NAMESPACE.to_string(),
            cr_type_meta: K8sConfig::default().cr_type_meta,
        };

        // The test fails due to the invalid kind here
        let assembler = get_agent_assembler_mock(sub_agent_config.clone(), agent_id.clone(), false);
        let builder = K8sSubAgentBuilder::new(
            Some(&opamp_builder),
            &instance_id_getter,
            Arc::new(MockSyncK8sClient::default()),
            Arc::new(hash_repository_mock),
            &assembler,
            &sub_agent_event_processor_builder,
            k8s_config,
        );

        let (opamp_publisher, _opamp_consumer) = pub_sub();
        let build_result = builder.build(agent_id, &sub_agent_config, opamp_publisher);
        let error = build_result.err().expect("Expected an error");
        assert_matches!(error, SubAgentBuilderError::UnsupportedK8sObject(_));
    }

    fn get_agent_assembler_mock(
        sub_agent_config: SubAgentConfig,
        agent_id: AgentID,
        valid_kind: bool,
    ) -> MockEffectiveAgentAssemblerMock {
        let effective_agent = EffectiveAgent::new(
            agent_id.clone(),
            Runtime {
                deployment: Deployment {
                    on_host: None,
                    k8s: Some(k8s_sample_runtime_config(valid_kind)),
                },
            },
        );

        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
        effective_agent_assembler.should_assemble_agent(
            &agent_id,
            &sub_agent_config,
            &Environment::K8s,
            effective_agent,
        );
        effective_agent_assembler
    }

    pub fn k8s_sample_runtime_config(valid_kind: bool) -> runtime_config::K8s {
        let kind = if valid_kind {
            "HelmRelease".to_string()
        } else {
            "UnsupportedKind".to_string()
        };

        let k8s_object = K8sObject {
            api_version: "helm.toolkit.fluxcd.io/v2beta2".to_string(),
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

    fn get_common_mocks(
        sub_agent_config: SubAgentConfig,
        agent_id: AgentID,
    ) -> (
        MockOpAMPClientBuilderMock<SubAgentCallbacks>,
        MockInstanceIDGetterMock,
        MockHashRepositoryMock,
    ) {
        let instance_id = "fake-ulid";

        // opamp builder mock
        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_any_remote_config_status(1);
        let mut opamp_builder = MockOpAMPClientBuilderMock::new();
        let start_settings = start_settings(
            instance_id.to_string(),
            &sub_agent_config.agent_type,
            HashMap::from([
                (
                    CLUSTER_NAME_ATTRIBUTE_KEY.to_string(),
                    DescriptionValueType::String(TEST_CLUSTER_NAME.to_string()),
                ),
                (
                    PARENT_AGENT_ID_ATTRIBUTE_KEY.to_string(),
                    DescriptionValueType::String("super_agent_instance_id".to_string()),
                ),
            ]),
        );
        opamp_builder.should_build_and_start(agent_id.clone(), start_settings, started_client);

        // instance id getter mock
        let mut instance_id_getter = MockInstanceIDGetterMock::new();
        instance_id_getter.should_get(&agent_id, instance_id.to_string());
        instance_id_getter.should_get(
            &AgentID::new_super_agent_id(),
            "super_agent_instance_id".to_string(),
        );

        // hash_repository_mock
        let mut hash_repository_mock = MockHashRepositoryMock::new();
        hash_repository_mock.expect_get().times(1).returning(|_| {
            let hash = Hash::new("a-hash".to_string());
            Ok(Some(hash))
        });
        hash_repository_mock
            .expect_save()
            .times(1)
            .returning(|_, _| Ok(()));

        (opamp_builder, instance_id_getter, hash_repository_mock)
    }
}
