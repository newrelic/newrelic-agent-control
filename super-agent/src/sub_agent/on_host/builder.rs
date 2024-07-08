use super::supervisor::command_supervisor_config::ExecutableData;
use super::{
    sub_agent::SubAgentOnHost,
    supervisor::{
        command_supervisor::SupervisorOnHost, command_supervisor_config::SupervisorConfigOnHost,
    },
};
use crate::agent_type::environment::Environment;
use crate::event::channel::{pub_sub, EventPublisher};
use crate::event::SubAgentEvent;
use crate::opamp::hash_repository::HashRepository;
use crate::opamp::instance_id::getter::InstanceIDGetter;
use crate::opamp::instance_id::IdentifiersProvider;
use crate::opamp::operations::build_sub_agent_opamp;
use crate::sub_agent::build_supervisor_or_default;
use crate::sub_agent::effective_agents_assembler::{EffectiveAgent, EffectiveAgentsAssembler};
use crate::sub_agent::event_processor_builder::SubAgentEventProcessorBuilder;
use crate::sub_agent::on_host::supervisor::command_supervisor;
use crate::sub_agent::on_host::supervisor::restart_policy::RestartPolicy;
use crate::sub_agent::values::values_repository::ValuesRepository;
use crate::sub_agent::NotStarted;
use crate::sub_agent::SubAgentCallbacks;
use crate::super_agent::config::{AgentID, SubAgentConfig};
use crate::super_agent::defaults::HOST_NAME_ATTRIBUTE_KEY;
use crate::{
    context::Context,
    opamp::client_builder::OpAMPClientBuilder,
    sub_agent::{
        error::{SubAgentBuilderError, SubAgentError},
        SubAgentBuilder,
    },
};
#[cfg(unix)]
use nix::unistd::gethostname;
use resource_detection::Detector;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;

pub struct OnHostSubAgentBuilder<'a, O, I, HR, A, E, R>
where
    R: ValuesRepository,
    O: OpAMPClientBuilder<SubAgentCallbacks<R>>,
    I: InstanceIDGetter,
    HR: HashRepository,
    A: EffectiveAgentsAssembler,
    E: SubAgentEventProcessorBuilder<O::Client, R>,
{
    opamp_builder: Option<&'a O>,
    instance_id_getter: &'a I,
    hash_repository: Arc<HR>,
    effective_agent_assembler: &'a A,
    event_processor_builder: &'a E,
    identifiers_provider: IdentifiersProvider,

    _phantom_r: PhantomData<R>,
}

impl<'a, O, I, HR, A, E, R> OnHostSubAgentBuilder<'a, O, I, HR, A, E, R>
where
    R: ValuesRepository,
    O: OpAMPClientBuilder<SubAgentCallbacks<R>>,
    I: InstanceIDGetter,
    HR: HashRepository,
    A: EffectiveAgentsAssembler,
    E: SubAgentEventProcessorBuilder<O::Client, R>,
{
    pub fn new(
        opamp_builder: Option<&'a O>,
        instance_id_getter: &'a I,
        hash_repository: Arc<HR>,
        effective_agent_assembler: &'a A,
        event_processor_builder: &'a E,
        identifiers_provider: IdentifiersProvider,
    ) -> Self {
        Self {
            opamp_builder,
            instance_id_getter,
            hash_repository,
            effective_agent_assembler,
            event_processor_builder,
            identifiers_provider,

            _phantom_r: PhantomData,
        }
    }
}

impl<'a, O, I, HR, A, E, R> SubAgentBuilder for OnHostSubAgentBuilder<'a, O, I, HR, A, E, R>
where
    R: ValuesRepository,
    O: OpAMPClientBuilder<SubAgentCallbacks<R>>,
    I: InstanceIDGetter,
    HR: HashRepository,
    A: EffectiveAgentsAssembler,
    E: SubAgentEventProcessorBuilder<O::Client, R>,
{
    type NotStartedSubAgent =
        SubAgentOnHost<NotStarted<E::SubAgentEventProcessor>, command_supervisor::NotStarted>;

    fn build(
        &self,
        agent_id: AgentID,
        sub_agent_config: &SubAgentConfig,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
    ) -> Result<Self::NotStartedSubAgent, SubAgentBuilderError> {
        let (sub_agent_internal_publisher, sub_agent_internal_consumer) = pub_sub();

        let (maybe_opamp_client, sub_agent_opamp_consumer) = self
            .opamp_builder
            .map(|builder| {
                build_sub_agent_opamp(
                    builder,
                    self.instance_id_getter,
                    agent_id.clone(),
                    &sub_agent_config.agent_type,
                    HashMap::from([(HOST_NAME_ATTRIBUTE_KEY().to_string(), get_hostname().into())]),
                )
            })
            // Transpose changes Option<Result<T, E>> to Result<Option<T>, E>, enabling the use of `?` to handle errors in this function
            .transpose()?
            .map(|(client, consumer)| (Some(client), Some(consumer)))
            .unwrap_or_default();

        // try to build effective agent
        let effective_agent_res = self.effective_agent_assembler.assemble_agent(
            &agent_id,
            sub_agent_config,
            &Environment::OnHost,
        );

        let supervisors = build_supervisor_or_default::<HR, O, _, _, _>(
            &agent_id,
            &self.hash_repository,
            &maybe_opamp_client,
            effective_agent_res,
            |effective_agent| build_supervisors(&self.identifiers_provider, effective_agent),
        )?;

        let event_processor = self.event_processor_builder.build(
            agent_id.clone(),
            sub_agent_publisher,
            sub_agent_opamp_consumer,
            sub_agent_internal_consumer,
            maybe_opamp_client,
        );

        Ok(SubAgentOnHost::new(
            agent_id,
            sub_agent_config.agent_type.clone(),
            supervisors,
            event_processor,
            sub_agent_internal_publisher,
        ))
    }
}

fn build_supervisors(
    identifiers_provider: &IdentifiersProvider,
    effective_agent: EffectiveAgent,
) -> Result<Vec<SupervisorOnHost<command_supervisor::NotStarted>>, SubAgentBuilderError> {
    let agent_id = effective_agent.get_agent_id();
    let on_host = effective_agent
        .get_runtime_config()
        .deployment
        .on_host
        .clone()
        .ok_or(SubAgentError::ErrorCreatingSubAgent(
            effective_agent.to_string(),
        ))?;

    let mut supervisors = Vec::new();
    let enable_file_logging = on_host.enable_file_logging.get();
    for exec in on_host.executables {
        let restart_policy: RestartPolicy = exec.restart_policy.into();
        let mut env = exec.env.get().into_map();
        env.extend(get_additional_env(identifiers_provider));

        let exec_data = ExecutableData::new(exec.path.get())
            .with_args(exec.args.get().into_vector())
            .with_env(env);

        let mut config = SupervisorConfigOnHost::new(
            agent_id.clone(),
            exec_data,
            Context::new(),
            restart_policy,
        )
        .with_file_logging(enable_file_logging);

        if let Some(health) = exec.health {
            config = config.with_health_check(health);
        }

        let not_started_supervisor = SupervisorOnHost::new(config);
        supervisors.push(not_started_supervisor);
    }
    Ok(supervisors)
}

fn get_hostname() -> String {
    #[cfg(unix)]
    return gethostname().unwrap_or_default().into_string().unwrap();

    #[cfg(not(unix))]
    return unimplemented!();
}

fn get_additional_env<D1, D2>(
    identifiers_provider: &IdentifiersProvider<D1, D2>,
) -> impl IntoIterator<Item = (String, String)>
where
    D1: Detector,
    D2: Detector,
{
    identifiers_provider
        .provide()
        .map(|ids| vec![("NR_HOST_ID".to_string(), ids.host_id)])
        .unwrap_or_default()
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::agent_type::runtime_config::{Deployment, OnHost, Runtime};
    use crate::event::channel::pub_sub;
    use crate::opamp::callbacks::AgentCallbacks;
    use crate::opamp::client_builder::test::MockOpAMPClientBuilderMock;
    use crate::opamp::client_builder::test::MockStartedOpAMPClientMock;
    use crate::opamp::hash_repository::repository::test::MockHashRepositoryMock;
    use crate::opamp::instance_id::getter::test::MockInstanceIDGetterMock;
    use crate::opamp::instance_id::test::{MockCloudDetectorMock, MockSystemDetectorMock};
    use crate::opamp::instance_id::InstanceID;
    use crate::opamp::remote_config_hash::Hash;
    use crate::sub_agent::effective_agents_assembler::tests::MockEffectiveAgentAssemblerMock;
    use crate::sub_agent::event_processor::test::MockEventProcessorMock;
    use crate::sub_agent::event_processor_builder::test::MockSubAgentEventProcessorBuilderMock;
    use crate::sub_agent::values::values_repository::test::MockRemoteValuesRepositoryMock;
    use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent};
    use crate::super_agent::config::AgentTypeFQN;
    use crate::super_agent::defaults::{default_capabilities, PARENT_AGENT_ID_ATTRIBUTE_KEY};
    use nix::unistd::gethostname;
    use opamp_client::opamp::proto::RemoteConfigStatus;
    use opamp_client::opamp::proto::RemoteConfigStatuses::Failed;
    use opamp_client::operation::settings::{
        AgentDescription, DescriptionValueType, StartSettings,
    };
    use resource_detection::cloud::cloud_id::detector::CloudIdDetectorError;
    use resource_detection::system::detector::SystemDetectorError;
    use resource_detection::{DetectError, Resource};
    use std::collections::HashMap;

    #[test]
    fn build_start_stop() {
        let (opamp_publisher, _opamp_consumer) = pub_sub();
        let mut opamp_builder: MockOpAMPClientBuilderMock<
            AgentCallbacks<MockRemoteValuesRepositoryMock>,
        > = MockOpAMPClientBuilderMock::new();
        let hostname = gethostname().unwrap_or_default().into_string().unwrap();
        let sub_agent_config = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("newrelic/com.newrelic.infrastructure_agent:0.0.2")
                .unwrap(),
        };

        let sub_agent_instance_id = InstanceID::create();
        let super_agent_instance_id = InstanceID::create();

        let start_settings_infra = infra_agent_default_start_settings(
            &hostname,
            super_agent_instance_id.clone(),
            sub_agent_instance_id.clone(),
            &sub_agent_config,
        );

        let super_agent_id = AgentID::new_super_agent_id();
        let sub_agent_id = AgentID::new("infra-agent").unwrap();
        let final_agent = on_host_final_agent(sub_agent_id.clone());

        let mut started_client: MockStartedOpAMPClientMock<_> = MockStartedOpAMPClientMock::new();
        started_client.should_set_any_remote_config_status(1);

        // Infra Agent OpAMP no final stop nor health, just after stopping on reload
        opamp_builder.should_build_and_start(
            sub_agent_id.clone(),
            start_settings_infra,
            started_client,
        );

        let mut instance_id_getter = MockInstanceIDGetterMock::new();
        instance_id_getter.should_get(&sub_agent_id, sub_agent_instance_id.clone());
        instance_id_getter.should_get(&super_agent_id, super_agent_instance_id.clone());

        let mut hash_repository_mock = MockHashRepositoryMock::new();
        hash_repository_mock.expect_get().times(1).returning(|_| {
            let hash = Hash::new("a-hash".to_string());
            Ok(Some(hash))
        });
        hash_repository_mock
            .expect_save()
            .times(1)
            .returning(|_, _| Ok(()));

        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
        effective_agent_assembler.should_assemble_agent(
            &sub_agent_id,
            &sub_agent_config,
            &Environment::OnHost,
            final_agent,
        );

        let mut sub_agent_event_processor_builder = MockSubAgentEventProcessorBuilderMock::new();
        sub_agent_event_processor_builder.should_return_event_processor_with_consumer();

        let on_host_builder = OnHostSubAgentBuilder::new(
            Some(&opamp_builder),
            &instance_id_getter,
            Arc::new(hash_repository_mock),
            &effective_agent_assembler,
            &sub_agent_event_processor_builder,
            IdentifiersProvider::default(),
        );

        assert!(on_host_builder
            .build(sub_agent_id, &sub_agent_config, opamp_publisher)
            .unwrap()
            .run()
            .stop()
            .is_ok())
    }

    #[test]
    fn test_builder_should_report_failed_config() {
        let (opamp_publisher, _opamp_consumer) = pub_sub();
        // Mocks
        let mut opamp_builder: MockOpAMPClientBuilderMock<
            AgentCallbacks<MockRemoteValuesRepositoryMock>,
        > = MockOpAMPClientBuilderMock::new();
        let mut hash_repository_mock = MockHashRepositoryMock::new();
        let mut instance_id_getter = MockInstanceIDGetterMock::new();
        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();

        // Structures
        let hostname = gethostname().unwrap_or_default().into_string().unwrap();
        let sub_agent_config = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("newrelic/com.newrelic.infrastructure_agent:0.0.2")
                .unwrap(),
        };
        let sub_agent_instance_id = InstanceID::create();
        let super_agent_instance_id = InstanceID::create();

        let start_settings_infra = infra_agent_default_start_settings(
            &hostname,
            super_agent_instance_id.clone(),
            sub_agent_instance_id.clone(),
            &sub_agent_config,
        );

        let super_agent_id = AgentID::new_super_agent_id();
        let sub_agent_id = AgentID::new("infra-agent").unwrap();
        let final_agent = on_host_final_agent(sub_agent_id.clone());
        // Expectations
        // Infra Agent OpAMP no final stop nor health, just after stopping on reload
        instance_id_getter.should_get(&sub_agent_id, sub_agent_instance_id.clone());
        instance_id_getter.should_get(&super_agent_id, super_agent_instance_id.clone());

        let mut started_client: MockStartedOpAMPClientMock<AgentCallbacks<_>> =
            MockStartedOpAMPClientMock::new();
        // failed conf should be reported
        started_client.should_set_remote_config_status(RemoteConfigStatus {
            error_message: "this is an error message".to_string(),
            status: Failed as i32,
            last_remote_config_hash: "a-hash".as_bytes().to_vec(),
        });

        opamp_builder.should_build_and_start(
            sub_agent_id.clone(),
            start_settings_infra,
            started_client,
        );

        effective_agent_assembler.should_assemble_agent(
            &sub_agent_id,
            &sub_agent_config,
            &Environment::OnHost,
            final_agent,
        );

        // return a failed hash
        let failed_hash =
            Hash::failed("a-hash".to_string(), "this is an error message".to_string());
        hash_repository_mock.should_get_hash(&sub_agent_id, failed_hash);

        let mut sub_agent_event_processor_builder = MockSubAgentEventProcessorBuilderMock::new();
        sub_agent_event_processor_builder.should_build(MockEventProcessorMock::default());

        // Sub Agent Builder
        let on_host_builder = OnHostSubAgentBuilder::new(
            Some(&opamp_builder),
            &instance_id_getter,
            Arc::new(hash_repository_mock),
            &effective_agent_assembler,
            &sub_agent_event_processor_builder,
            IdentifiersProvider::default(),
        );

        assert!(on_host_builder
            .build(sub_agent_id, &sub_agent_config, opamp_publisher)
            .is_ok());
    }

    // HELPERS
    #[cfg(test)]
    fn on_host_final_agent(agent_id: AgentID) -> EffectiveAgent {
        use crate::agent_type::definition::TemplateableValue;

        EffectiveAgent::new(
            agent_id,
            Runtime {
                deployment: Deployment {
                    on_host: Some(OnHost {
                        executables: Vec::new(),
                        enable_file_logging: TemplateableValue::new(false),
                    }),
                    k8s: None,
                },
            },
        )
    }

    fn infra_agent_default_start_settings(
        hostname: &str,
        super_agent_instance_id: InstanceID,
        sub_agent_instance_id: InstanceID,
        agent_config: &SubAgentConfig,
    ) -> StartSettings {
        StartSettings {
            instance_id: sub_agent_instance_id.into(),
            capabilities: default_capabilities(),
            agent_description: AgentDescription {
                identifying_attributes: HashMap::<String, DescriptionValueType>::from([
                    (
                        "service.name".to_string(),
                        agent_config.agent_type.name().into(),
                    ),
                    (
                        "service.namespace".to_string(),
                        agent_config.agent_type.namespace().into(),
                    ),
                    (
                        "service.version".to_string(),
                        agent_config.agent_type.version().into(),
                    ),
                ]),
                non_identifying_attributes: HashMap::from([
                    (
                        HOST_NAME_ATTRIBUTE_KEY().to_string(),
                        DescriptionValueType::String(hostname.to_string()),
                    ),
                    (
                        PARENT_AGENT_ID_ATTRIBUTE_KEY().to_string(),
                        DescriptionValueType::Bytes(super_agent_instance_id.into()),
                    ),
                ]),
            },
        }
    }

    #[test]
    fn build_additional_env_from_system_provider_empty_cloud() {
        let mut system_detector = MockSystemDetectorMock::default();
        let mut cloud_detector = MockCloudDetectorMock::default();
        let system_resource = Resource::new([(
            "machine_id".to_string().into(),
            "some machine id".to_string().into(),
        )]);
        let cloud_resource = Resource::new([]);

        system_detector.should_detect(system_resource);
        cloud_detector.should_detect(cloud_resource);

        let expected = HashMap::from([("NR_HOST_ID".to_string(), "some machine id".to_string())]);

        let identifiers_provider = IdentifiersProvider::new(system_detector, cloud_detector);
        let actual = get_additional_env(&identifiers_provider)
            .into_iter()
            .collect();

        assert_eq!(expected, actual);
    }

    #[test]
    fn build_additional_env_from_system_provider_with_cloud() {
        let mut system_detector = MockSystemDetectorMock::default();
        let mut cloud_detector = MockCloudDetectorMock::default();
        let system_resource = Resource::new([(
            "machine_id".to_string().into(),
            "some machine id".to_string().into(),
        )]);
        let cloud_resource = Resource::new([(
            "cloud_instance_id".to_string().into(),
            "some cloud id".to_string().into(),
        )]);

        system_detector.should_detect(system_resource);
        cloud_detector.should_detect(cloud_resource);

        let expected = HashMap::from([("NR_HOST_ID".to_string(), "some cloud id".to_string())]);

        let identifiers_provider = IdentifiersProvider::new(system_detector, cloud_detector);
        let actual = get_additional_env(&identifiers_provider)
            .into_iter()
            .collect();

        assert_eq!(expected, actual);
    }

    #[test]
    fn build_additional_env_with_empty_but_valid_detection() {
        let mut system_detector = MockSystemDetectorMock::default();
        let mut cloud_detector = MockCloudDetectorMock::default();
        let system_resource = Resource::new([]);
        let cloud_resource = Resource::new([]);

        system_detector.should_detect(system_resource);
        cloud_detector.should_detect(cloud_resource);

        let expected = HashMap::new();

        let identifiers_provider = IdentifiersProvider::new(system_detector, cloud_detector);
        let actual = get_additional_env(&identifiers_provider)
            .into_iter()
            .collect();

        assert_eq!(expected, actual);
    }

    #[test]
    fn build_additional_env_with_failing_system_detection_does_not_detect_cloud() {
        let mut system_detector = MockSystemDetectorMock::default();
        let mut cloud_detector = MockCloudDetectorMock::default();
        let system_detection_err =
            DetectError::SystemError(SystemDetectorError::HostnameError("random err".into()));

        system_detector.should_fail_detection(system_detection_err);
        cloud_detector.expect_detect().never();

        let expected = HashMap::from([]);

        let identifiers_provider = IdentifiersProvider::new(system_detector, cloud_detector);
        let actual = get_additional_env(&identifiers_provider)
            .into_iter()
            .collect();

        assert_eq!(expected, actual);
    }

    #[test]
    fn build_additional_env_with_failing_cloud_detection() {
        let mut system_detector = MockSystemDetectorMock::default();
        let mut cloud_detector = MockCloudDetectorMock::default();
        let system_resource = Resource::new([]);
        let cloud_detection_err =
            DetectError::CloudIdError(CloudIdDetectorError::UnsuccessfulCloudIdCheck());

        system_detector.should_detect(system_resource);
        cloud_detector.should_fail_detection(cloud_detection_err);

        let expected = HashMap::new();

        let identifiers_provider = IdentifiersProvider::new(system_detector, cloud_detector);
        let actual = get_additional_env(&identifiers_provider)
            .into_iter()
            .collect();

        assert_eq!(expected, actual);
    }
}
