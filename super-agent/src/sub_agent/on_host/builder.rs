use std::collections::HashMap;
use std::sync::Arc;

#[cfg(unix)]
use nix::unistd::gethostname;

use crate::config::super_agent_configs::SubAgentConfig;
use crate::event::channel::{pub_sub, EventPublisher};
use crate::event::SubAgentEvent;
use crate::opamp::instance_id::getter::InstanceIDGetter;
use crate::opamp::operations::build_opamp_and_start_client;
use crate::opamp::remote_config_hash::HashRepository;
use crate::opamp::remote_config_report::{
    report_remote_config_status_applied, report_remote_config_status_error,
};

use crate::sub_agent::on_host::event_processor_builder::SubAgentEventProcessorBuilder;
use crate::sub_agent::on_host::sub_agent::NotStarted;
use crate::sub_agent::on_host::supervisor::command_supervisor;
use crate::sub_agent::SubAgentCallbacks;
use crate::super_agent::effective_agents_assembler::{
    EffectiveAgentsAssembler, EffectiveAgentsAssemblerError,
};
use crate::{
    config::{agent_type::agent_types::FinalAgent, super_agent_configs::AgentID},
    context::Context,
    opamp::client_builder::OpAMPClientBuilder,
    sub_agent::{
        error::{SubAgentBuilderError, SubAgentError},
        logger::AgentLog,
        restart_policy::RestartPolicy,
        SubAgentBuilder,
    },
};
use log::error;
use EffectiveAgentsAssemblerError::RemoteConfigLoadError;

use super::{
    sub_agent::SubAgentOnHost,
    supervisor::{
        command_supervisor::SupervisorOnHost, command_supervisor_config::SupervisorConfigOnHost,
    },
};

pub struct OnHostSubAgentBuilder<'a, O, I, HR, A, E>
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
    effective_agent_assembler: &'a A,
    event_processor_builder: &'a E,
}

impl<'a, O, I, HR, A, E> OnHostSubAgentBuilder<'a, O, I, HR, A, E>
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
        hash_repository: Arc<HR>,
        effective_agent_assembler: &'a A,
        event_processor_builder: &'a E,
    ) -> Self {
        Self {
            opamp_builder,
            instance_id_getter,
            hash_repository,
            effective_agent_assembler,
            event_processor_builder,
        }
    }
}

impl<'a, O, I, HR, A, E> SubAgentBuilder for OnHostSubAgentBuilder<'a, O, I, HR, A, E>
where
    O: OpAMPClientBuilder<SubAgentCallbacks>,
    I: InstanceIDGetter,
    HR: HashRepository,
    A: EffectiveAgentsAssembler,
    E: SubAgentEventProcessorBuilder<O::Client>,
{
    type NotStartedSubAgent = SubAgentOnHost<
        NotStarted<O::Client, E::SubAgentEventProcessor>,
        command_supervisor::NotStarted,
    >;

    fn build(
        &self,
        agent_id: AgentID,
        sub_agent_config: &SubAgentConfig,
        tx: std::sync::mpsc::Sender<AgentLog>,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
    ) -> Result<Self::NotStartedSubAgent, SubAgentBuilderError> {
        let (sub_agent_opamp_publisher, sub_agent_opamp_consumer) = pub_sub();

        let maybe_opamp_client = build_opamp_and_start_client(
            sub_agent_opamp_publisher,
            self.opamp_builder,
            self.instance_id_getter,
            agent_id.clone(),
            &sub_agent_config.agent_type,
            HashMap::from([("host.name".to_string(), get_hostname().into())]),
        )?;

        // try to build effective agent
        let effective_agent_res = self
            .effective_agent_assembler
            .assemble_agent(&agent_id, sub_agent_config);

        let mut has_supervisors = true;

        if let Some(opamp_client) = &maybe_opamp_client {
            let remote_config_hash = self
                .hash_repository
                .get(&agent_id)
                .map_err(|e| error!("hash repository error: {}", e))
                .ok();

            if let Some(mut hash) = remote_config_hash {
                // send to opamp the remote config error in case it happens
                if let Err(RemoteConfigLoadError(error)) = effective_agent_res.as_ref() {
                    report_remote_config_status_error(opamp_client, &hash, error.clone())?;
                    // report the failed status for remote config and let the opamp client
                    // running with no supervisors so the configuration can be fixed
                    has_supervisors = false;
                } else if hash.is_applying() {
                    report_remote_config_status_applied(opamp_client, &hash)?;
                    hash.apply();
                    self.hash_repository.save(&agent_id, &hash)?;
                } else if hash.is_failed() {
                    // failed hash always has the error message
                    let error_message = hash.error_message().unwrap();
                    report_remote_config_status_error(
                        opamp_client,
                        &hash,
                        error_message.to_string(),
                    )?;
                }
            }
        }

        let supervisors = match has_supervisors {
            false => Vec::new(),
            true => build_supervisors(effective_agent_res?, tx)?,
        };

        let event_processor = self.event_processor_builder.build(
            sub_agent_publisher,
            sub_agent_opamp_consumer,
            maybe_opamp_client,
        );

        Ok(SubAgentOnHost::new(agent_id, supervisors, event_processor))
    }
}

fn build_supervisors(
    final_agent: FinalAgent,
    tx: std::sync::mpsc::Sender<AgentLog>,
) -> Result<Vec<SupervisorOnHost<command_supervisor::NotStarted>>, SubAgentError> {
    let on_host = final_agent
        .runtime_config
        .deployment
        .on_host
        .clone()
        .ok_or(SubAgentError::ErrorCreatingSubAgent(
            final_agent.agent_type().to_string(),
        ))?;

    let mut supervisors = Vec::new();
    for exec in on_host.executables {
        let restart_policy: RestartPolicy = exec.restart_policy.into();
        let config = SupervisorConfigOnHost::new(
            exec.path.get(),
            exec.args.get().into_vector(),
            Context::new(),
            exec.env.get().into_map(),
            tx.clone(),
            restart_policy,
        );

        let not_started_supervisor = SupervisorOnHost::new(config);
        supervisors.push(not_started_supervisor);
    }
    Ok(supervisors)
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;
    use std::sync::mpsc::channel;

    use nix::unistd::gethostname;
    use opamp_client::opamp::proto::RemoteConfigStatus;
    use opamp_client::opamp::proto::RemoteConfigStatuses::Failed;
    use opamp_client::operation::{
        capabilities::Capabilities,
        settings::{AgentDescription, DescriptionValueType, StartSettings},
    };

    use crate::event::channel::pub_sub;
    use crate::opamp::client_builder::test::MockStartedOpAMPClientMock;
    use crate::opamp::instance_id::getter::test::MockInstanceIDGetterMock;
    use crate::opamp::remote_config_hash::test::MockHashRepositoryMock;
    use crate::opamp::remote_config_hash::Hash;
    use crate::sub_agent::on_host::event_processor::test::MockEventProcessorMock;
    use crate::sub_agent::on_host::event_processor_builder::test::MockSubAgentEventProcessorBuilderMock;
    use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent};
    use crate::{
        config::agent_type::runtime_config::OnHost,
        opamp::client_builder::test::MockOpAMPClientBuilderMock,
    };

    use super::*;

    use crate::super_agent::defaults::default_capabilities;
    use crate::super_agent::effective_agents_assembler::tests::MockEffectiveAgentAssemblerMock;

    #[test]
    fn build_start_stop() {
        let (opamp_publisher, _opamp_consumer) = pub_sub();
        let mut opamp_builder = MockOpAMPClientBuilderMock::new();
        let hostname = gethostname().unwrap_or_default().into_string().unwrap();
        let start_settings_infra = infra_agent_default_start_settings(&hostname);

        let final_agent = on_host_final_agent();
        let sub_agent_id = AgentID::new("infra_agent").unwrap();
        let sub_agent_config = SubAgentConfig {
            agent_type: final_agent.agent_type().clone(),
        };

        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_any_remote_config_status(1);

        // Infra Agent OpAMP no final stop nor health, just after stopping on reload
        opamp_builder.should_build_and_start(
            sub_agent_id.clone(),
            start_settings_infra,
            started_client,
        );

        let mut instance_id_getter = MockInstanceIDGetterMock::new();
        instance_id_getter.should_get(&sub_agent_id, "infra_agent_instance_id".to_string());

        let mut hash_repository_mock = MockHashRepositoryMock::new();
        hash_repository_mock.expect_get().times(1).returning(|_| {
            let hash = Hash::new("a-hash".to_string());
            Ok(hash)
        });
        hash_repository_mock
            .expect_save()
            .times(1)
            .returning(|_, _| Ok(()));

        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
        effective_agent_assembler.should_assemble_agent(
            &sub_agent_id,
            &sub_agent_config,
            final_agent,
        );

        let mut sub_agent_event_processor: MockEventProcessorMock<
            MockStartedOpAMPClientMock<SubAgentCallbacks>,
        > = MockEventProcessorMock::default();

        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_stop(1);
        started_client.should_set_health(1);
        sub_agent_event_processor.should_process(Some(started_client));

        let mut sub_agent_event_processor_builder = MockSubAgentEventProcessorBuilderMock::new();
        sub_agent_event_processor_builder.should_build(sub_agent_event_processor);

        let on_host_builder = OnHostSubAgentBuilder::new(
            Some(&opamp_builder),
            &instance_id_getter,
            Arc::new(hash_repository_mock),
            &effective_agent_assembler,
            &sub_agent_event_processor_builder,
        );

        let (tx, _rx) = channel();

        assert!(on_host_builder
            .build(sub_agent_id, &sub_agent_config, tx, opamp_publisher)
            .unwrap()
            .run()
            .unwrap()
            .stop()
            .is_ok())
    }

    #[test]
    fn test_builder_should_report_failed_config() {
        let (opamp_publisher, _opamp_consumer) = pub_sub();
        let (tx, _rx) = channel();
        // Mocks
        let mut opamp_builder = MockOpAMPClientBuilderMock::new();
        let mut hash_repository_mock = MockHashRepositoryMock::new();
        let mut instance_id_getter = MockInstanceIDGetterMock::new();
        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();

        // Structures
        let hostname = gethostname().unwrap_or_default().into_string().unwrap();
        let start_settings_infra = infra_agent_default_start_settings(&hostname);
        let final_agent = on_host_final_agent();
        let sub_agent_id = AgentID::new("infra_agent").unwrap();
        let sub_agent_config = SubAgentConfig {
            agent_type: final_agent.agent_type().clone(),
        };

        // Expectations
        // Infra Agent OpAMP no final stop nor health, just after stopping on reload
        instance_id_getter.should_get(&sub_agent_id, "infra_agent_instance_id".to_string());

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
            final_agent,
        );

        // return a failed hash
        let failed_hash =
            Hash::failed("a-hash".to_string(), "this is an error message".to_string());
        hash_repository_mock.should_get_hash(&sub_agent_id, failed_hash);

        let sub_agent_event_processor: MockEventProcessorMock<
            MockStartedOpAMPClientMock<SubAgentCallbacks>,
        > = MockEventProcessorMock::default();

        let mut sub_agent_event_processor_builder = MockSubAgentEventProcessorBuilderMock::new();
        sub_agent_event_processor_builder.should_build(sub_agent_event_processor);

        // Sub Agent Builder
        let on_host_builder = OnHostSubAgentBuilder::new(
            Some(&opamp_builder),
            &instance_id_getter,
            Arc::new(hash_repository_mock),
            &effective_agent_assembler,
            &sub_agent_event_processor_builder,
        );

        assert!(on_host_builder
            .build(sub_agent_id, &sub_agent_config, tx, opamp_publisher)
            .is_ok());
    }

    // HELPERS
    fn on_host_final_agent() -> FinalAgent {
        let mut final_agent: FinalAgent = FinalAgent::default();
        final_agent.runtime_config.deployment.on_host = Some(OnHost {
            executables: Vec::new(),
        });
        final_agent
    }

    fn infra_agent_default_start_settings(hostname: &str) -> StartSettings {
        start_settings(
            "infra_agent_instance_id".to_string(),
            default_capabilities(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            hostname,
        )
    }

    fn start_settings(
        instance_id: String,
        capabilities: Capabilities,
        agent_type: String,
        agent_version: String,
        agent_namespace: String,
        hostname: &str,
    ) -> StartSettings {
        StartSettings {
            instance_id,
            capabilities,
            agent_description: AgentDescription {
                identifying_attributes: HashMap::<String, DescriptionValueType>::from([
                    ("service.name".to_string(), agent_type.into()),
                    ("service.namespace".to_string(), agent_namespace.into()),
                    ("service.version".to_string(), agent_version.into()),
                ]),
                non_identifying_attributes: HashMap::from([(
                    "host.name".to_string(),
                    DescriptionValueType::String(hostname.to_string()),
                )]),
            },
        }
    }
}

fn get_hostname() -> String {
    #[cfg(unix)]
    return gethostname().unwrap_or_default().into_string().unwrap();

    #[cfg(not(unix))]
    return unimplemented!();
}
