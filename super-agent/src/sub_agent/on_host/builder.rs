use super::health_checker::{HealthChecker, HealthCheckerNotStarted};
use super::supervisor::command_supervisor_config::ExecutableData;
use super::{
    sub_agent::SubAgentOnHost,
    supervisor::{
        command_supervisor::SupervisorOnHost, command_supervisor_config::SupervisorConfigOnHost,
    },
};
use crate::agent_type::environment::Environment;
use crate::agent_type::runtime_config::Executable;
use crate::event::channel::{pub_sub, EventPublisher};
use crate::event::SubAgentEvent;
use crate::opamp::effective_config::loader::EffectiveConfigLoader;
use crate::opamp::hash_repository::HashRepository;
use crate::opamp::instance_id::getter::InstanceIDGetter;
use crate::opamp::operations::build_sub_agent_opamp;
use crate::sub_agent::build_supervisor_or_default;
use crate::sub_agent::effective_agents_assembler::{EffectiveAgent, EffectiveAgentsAssembler};
use crate::sub_agent::event_processor_builder::SubAgentEventProcessorBuilder;
use crate::sub_agent::on_host::supervisor::command_supervisor;
use crate::sub_agent::on_host::supervisor::restart_policy::RestartPolicy;
use crate::sub_agent::NotStarted;
use crate::sub_agent::SubAgentCallbacks;
use crate::super_agent::config::{AgentID, SubAgentConfig};
use crate::super_agent::defaults::HOST_NAME_ATTRIBUTE_KEY;
use crate::{
    context::Context,
    opamp::client_builder::OpAMPClientBuilder,
    sub_agent::{error::SubAgentBuilderError, SubAgentBuilder},
};
#[cfg(unix)]
use nix::unistd::gethostname;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::error;

pub struct OnHostSubAgentBuilder<'a, O, I, HR, A, E, G>
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
    effective_agent_assembler: &'a A,
    event_processor_builder: &'a E,
    logging_path: PathBuf,

    // This is needed to ensure the generic type parameter G is used in the struct.
    // Else Rust will reject this, complaining that the type parameter is not used.
    _effective_config_loader: PhantomData<G>,
}

impl<'a, O, I, HR, A, E, G> OnHostSubAgentBuilder<'a, O, I, HR, A, E, G>
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
        hash_repository: Arc<HR>,
        effective_agent_assembler: &'a A,
        event_processor_builder: &'a E,
        logging_path: PathBuf,
    ) -> Self {
        Self {
            opamp_builder,
            instance_id_getter,
            hash_repository,
            effective_agent_assembler,
            event_processor_builder,
            logging_path,

            _effective_config_loader: PhantomData,
        }
    }

    fn build_supervisors(
        &self,
        effective_agent: EffectiveAgent,
    ) -> Result<Vec<SupervisorOnHost<command_supervisor::NotStarted>>, SubAgentBuilderError> {
        let agent_id = effective_agent.get_agent_id();
        let on_host = effective_agent.get_onhost_config()?.clone();

        let enable_file_logging = on_host.enable_file_logging.get();
        let supervisors = on_host
            .executables
            .into_iter()
            .map(|e| self.create_executable_supervisor(agent_id.clone(), enable_file_logging, e))
            .collect();

        Ok(supervisors)
    }

    fn create_executable_supervisor(
        &self,
        agent_id: AgentID,
        enable_file_logging: bool,
        executable: Executable,
    ) -> SupervisorOnHost<command_supervisor::NotStarted> {
        let restart_policy: RestartPolicy = executable.restart_policy.into();
        let env = executable.env.get();

        let exec_data = ExecutableData::new(executable.path.get())
            .with_args(executable.args.get().into_vector())
            .with_env(env);

        let config =
            SupervisorConfigOnHost::new(agent_id, exec_data, Context::new(), restart_policy)
                .with_file_logging(enable_file_logging, self.logging_path.to_path_buf());

        SupervisorOnHost::new(config)
    }
}

impl<'a, O, I, HR, A, E, G> SubAgentBuilder for OnHostSubAgentBuilder<'a, O, I, HR, A, E, G>
where
    G: EffectiveConfigLoader,
    O: OpAMPClientBuilder<SubAgentCallbacks<G>>,
    I: InstanceIDGetter,
    HR: HashRepository,
    A: EffectiveAgentsAssembler,
    E: SubAgentEventProcessorBuilder<O::Client, G>,
{
    type NotStartedSubAgent = SubAgentOnHost<
        NotStarted<E::SubAgentEventProcessor>,
        command_supervisor::NotStarted,
        HealthCheckerNotStarted,
    >;

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
                    HashMap::from([(HOST_NAME_ATTRIBUTE_KEY.to_string(), get_hostname().into())]),
                )
            })
            // Transpose changes Option<Result<T, E>> to Result<Option<T>, E>, enabling the use of `?` to handle errors in this function
            .transpose()?
            .map(|(client, consumer)| (Some(client), Some(consumer)))
            .unwrap_or_default();

        let agent_fqn = sub_agent_config.agent_type.clone();
        // try to build effective agent
        let effective_agent_res = self.effective_agent_assembler.assemble_agent(
            &agent_id,
            sub_agent_config,
            &Environment::OnHost,
        );

        // try to build health checker
        let health_checker = match &effective_agent_res {
            Ok(effective_agent) => effective_agent
                .get_onhost_config()?
                .health
                .as_ref()
                .and_then(|health_config| {
                    HealthChecker::try_new(
                        agent_id.clone(),
                        sub_agent_internal_publisher.clone(),
                        health_config.clone(),
                    )
                    .inspect_err(|err| {
                        error!(
                            %agent_id,
                            %err,
                            "could not launch health checker, using default",
                        )
                    })
                    .ok()
                }),
            _ => None,
        };

        let supervisors = build_supervisor_or_default::<HR, O, _, _, _>(
            &agent_id,
            &self.hash_repository,
            &maybe_opamp_client,
            effective_agent_res,
            |effective_agent| self.build_supervisors(effective_agent),
        )?;

        let event_processor = self.event_processor_builder.build(
            agent_id.clone(),
            agent_fqn,
            sub_agent_publisher,
            sub_agent_opamp_consumer,
            sub_agent_internal_consumer,
            maybe_opamp_client,
        );

        Ok(SubAgentOnHost::new(
            agent_id,
            sub_agent_config.agent_type.clone(),
            health_checker,
            supervisors,
            event_processor,
            sub_agent_internal_publisher,
        ))
    }
}

fn get_hostname() -> String {
    #[cfg(unix)]
    return gethostname().unwrap_or_default().into_string().unwrap();

    #[cfg(not(unix))]
    return unimplemented!();
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::agent_type::runtime_config::{Deployment, OnHost, Runtime};
    use crate::event::channel::pub_sub;
    use crate::opamp::client_builder::test::MockOpAMPClientBuilderMock;
    use crate::opamp::client_builder::test::MockStartedOpAMPClientMock;
    use crate::opamp::hash_repository::repository::test::MockHashRepositoryMock;
    use crate::opamp::instance_id::getter::test::MockInstanceIDGetterMock;
    use crate::opamp::instance_id::InstanceID;
    use crate::opamp::remote_config_hash::Hash;
    use crate::sub_agent::effective_agents_assembler::tests::MockEffectiveAgentAssemblerMock;
    use crate::sub_agent::event_processor::test::MockEventProcessorMock;
    use crate::sub_agent::event_processor_builder::test::MockSubAgentEventProcessorBuilderMock;
    use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent};
    use crate::super_agent::config::AgentTypeFQN;
    use crate::super_agent::defaults::{default_capabilities, PARENT_AGENT_ID_ATTRIBUTE_KEY};
    use nix::unistd::gethostname;
    use opamp_client::opamp::proto::RemoteConfigStatus;
    use opamp_client::opamp::proto::RemoteConfigStatuses::Failed;
    use opamp_client::operation::settings::{
        AgentDescription, DescriptionValueType, StartSettings,
    };
    use std::collections::HashMap;

    #[test]
    fn build_start_stop() {
        let (opamp_publisher, _opamp_consumer) = pub_sub();
        let mut opamp_builder = MockOpAMPClientBuilderMock::new();
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
        let final_agent =
            on_host_final_agent(sub_agent_id.clone(), sub_agent_config.agent_type.clone());

        let mut started_client = MockStartedOpAMPClientMock::new();
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
            PathBuf::default(),
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
        let mut opamp_builder = MockOpAMPClientBuilderMock::new();
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
        let final_agent =
            on_host_final_agent(sub_agent_id.clone(), sub_agent_config.agent_type.clone());
        // Expectations
        // Infra Agent OpAMP no final stop nor health, just after stopping on reload
        instance_id_getter.should_get(&sub_agent_id, sub_agent_instance_id.clone());
        instance_id_getter.should_get(&super_agent_id, super_agent_instance_id.clone());

        let mut started_client = MockStartedOpAMPClientMock::new();
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
            PathBuf::default(),
        );

        assert!(on_host_builder
            .build(sub_agent_id, &sub_agent_config, opamp_publisher)
            .is_ok());
    }

    // HELPERS
    #[cfg(test)]
    fn on_host_final_agent(agent_id: AgentID, agent_fqn: AgentTypeFQN) -> EffectiveAgent {
        use crate::agent_type::definition::TemplateableValue;

        EffectiveAgent::new(
            agent_id,
            agent_fqn,
            Runtime {
                deployment: Deployment {
                    on_host: Some(OnHost {
                        executables: vec![],
                        enable_file_logging: TemplateableValue::new(false),
                        health: None,
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
                        HOST_NAME_ATTRIBUTE_KEY.to_string(),
                        DescriptionValueType::String(hostname.to_string()),
                    ),
                    (
                        PARENT_AGENT_ID_ATTRIBUTE_KEY.to_string(),
                        DescriptionValueType::Bytes(super_agent_instance_id.into()),
                    ),
                ]),
            },
        }
    }
}
