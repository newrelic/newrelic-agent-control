use crate::config::remote_config_hash::{Hash, HashRepository};
use crate::config::super_agent_configs::AgentTypeFQN;
use crate::sub_agent::on_host::opamp::build_opamp_and_start_client;
use crate::super_agent::effective_agents_assembler::EffectiveAgentsAssemblerError;
use crate::{
    config::{agent_type::agent_types::FinalAgent, super_agent_configs::AgentID},
    context::Context,
    opamp::client_builder::OpAMPClientBuilder,
    sub_agent::{
        error::{SubAgentBuilderError, SubAgentError},
        logger::Event,
        restart_policy::RestartPolicy,
        SubAgentBuilder,
    },
    super_agent::instance_id::InstanceIDGetter,
};
use futures::executor::block_on;
use log::error;
use opamp_client::opamp::proto::{RemoteConfigStatus, RemoteConfigStatuses};
use opamp_client::Client;

use super::{
    sub_agent::NotStartedSubAgentOnHost,
    supervisor::{
        command_supervisor::NotStartedSupervisorOnHost,
        command_supervisor_config::SupervisorConfigOnHost,
    },
};

pub struct OnHostSubAgentBuilder<'a, O, I, HR>
where
    O: OpAMPClientBuilder,
    I: InstanceIDGetter,
{
    opamp_builder: Option<&'a O>,
    instance_id_getter: &'a I,
    hash_repository: &'a HR,
}

impl<'a, O, I, HR> OnHostSubAgentBuilder<'a, O, I, HR>
where
    O: OpAMPClientBuilder,
    I: InstanceIDGetter,
    HR: HashRepository,
{
    pub fn new(
        opamp_builder: Option<&'a O>,
        instance_id_getter: &'a I,
        hash_repository: &'a HR,
    ) -> Self {
        Self {
            opamp_builder,
            instance_id_getter,
            hash_repository,
        }
    }
}

impl<'a, O, I, HR> SubAgentBuilder for OnHostSubAgentBuilder<'a, O, I, HR>
where
    O: OpAMPClientBuilder,
    I: InstanceIDGetter,
    HR: HashRepository,
{
    type NotStartedSubAgent = NotStartedSubAgentOnHost<O::Client>;
    fn build(
        &self,
        agent: Result<FinalAgent, EffectiveAgentsAssemblerError>,
        agent_id: AgentID,
        agent_fqn: &AgentTypeFQN,
        tx: std::sync::mpsc::Sender<Event>,
    ) -> Result<Self::NotStartedSubAgent, SubAgentBuilderError> {
        let opamp_client = build_opamp_and_start_client(
            Context::new(),
            self.opamp_builder,
            self.instance_id_getter,
            agent_id.clone(),
            agent_fqn,
        )?;

        if let Some(handle) = &opamp_client {
            let remote_config_hash = self
                .hash_repository
                .get(&agent_id)
                .map_err(|e| error!("hash repository error: {}", e))
                .ok();

            if let Some(hash) = remote_config_hash {
                if !hash.is_applied() {
                    self.apply_hash_and_send_opamp(hash, &agent_id, &agent, handle)?;
                }
            }
        }

        // If there was no final agent we propagate the error
        let final_agent = agent?;

        Ok(NotStartedSubAgentOnHost::new(
            agent_id,
            build_supervisors(final_agent, tx)?,
            opamp_client,
        )?)
    }
    // add code here
}

impl<'a, O, I, HR> OnHostSubAgentBuilder<'a, O, I, HR>
where
    O: OpAMPClientBuilder,
    I: InstanceIDGetter,
    HR: HashRepository,
{
    /// Sets the applied flag from the remote_config_hash repository to true
    /// and sends the remote_config_status to opamp server
    fn apply_hash_and_send_opamp(
        &self,
        mut hash: Hash,
        agent_id: &AgentID,
        agent: &Result<FinalAgent, EffectiveAgentsAssemblerError>,
        opamp_client: &O::Client,
    ) -> Result<(), SubAgentBuilderError> {
        let mut remote_config_status = RemoteConfigStatus::default();
        match agent {
            Ok(_) => {
                remote_config_status.last_remote_config_hash = hash.get().into_bytes();
                remote_config_status.status = RemoteConfigStatuses::Applied as i32;
            }
            Err(e) => {
                remote_config_status.last_remote_config_hash = hash.get().into_bytes();
                remote_config_status.status = RemoteConfigStatuses::Failed as i32;
                remote_config_status.error_message = e.to_string();
            }
        }

        block_on(opamp_client.set_remote_config_status(remote_config_status))?;
        hash.apply();
        self.hash_repository.save(agent_id, &hash)?;

        Ok(())
    }
}

fn build_supervisors(
    final_agent: FinalAgent,
    tx: std::sync::mpsc::Sender<Event>,
) -> Result<Vec<NotStartedSupervisorOnHost>, SubAgentError> {
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

        let not_started_supervisor = NotStartedSupervisorOnHost::new(config);
        supervisors.push(not_started_supervisor);
    }
    Ok(supervisors)
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;
    use std::sync::mpsc::channel;

    use nix::unistd::gethostname;
    use opamp_client::opamp::proto::AgentCapabilities;
    use opamp_client::{
        capabilities,
        operation::{
            capabilities::Capabilities,
            settings::{AgentDescription, DescriptionValueType, StartSettings},
        },
    };

    use crate::config::remote_config_hash::test::MockHashRepositoryMock;
    use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent};
    use crate::{
        config::agent_type::runtime_config::OnHost,
        opamp::client_builder::test::{MockOpAMPClientBuilderMock, MockOpAMPClientMock},
        super_agent::instance_id::test::MockInstanceIDGetterMock,
    };

    use super::*;

    #[test]
    fn build_start_stop() {
        let mut opamp_builder = MockOpAMPClientBuilderMock::new();
        let hostname = gethostname().unwrap_or_default().into_string().unwrap();
        let start_settings_infra = infra_agent_default_start_settings(&hostname);

        // Infra Agent OpAMP no final stop nor health, just after stopping on reload
        opamp_builder.should_build_and_start(
            AgentID::new("infra_agent").unwrap(),
            start_settings_infra,
            |_, _, _| {
                let mut started_client = MockOpAMPClientMock::new();
                started_client.should_set_health(1);
                started_client.should_set_remote_config_status(1);
                started_client.should_stop(1);
                Ok(started_client)
            },
        );

        let mut instance_id_getter = MockInstanceIDGetterMock::new();
        instance_id_getter.should_get(
            "infra_agent".to_string(),
            "infra_agent_instance_id".to_string(),
        );

        let mut hash_repository_mock = MockHashRepositoryMock::new();
        hash_repository_mock.expect_get().times(1).returning(|_| {
            let hash = Hash::new("a-hash".to_string());
            Ok(hash)
        });
        hash_repository_mock
            .expect_save()
            .times(1)
            .returning(|_, _| Ok(()));

        let on_host_builder = OnHostSubAgentBuilder::new(
            Some(&opamp_builder),
            &instance_id_getter,
            &hash_repository_mock,
        );

        let (tx, _rx) = channel();

        let final_agent = on_host_final_agent();
        assert!(on_host_builder
            .build(
                Ok(final_agent.clone()),
                AgentID::new("infra_agent").unwrap(),
                &final_agent.agent_type(),
                tx
            )
            .unwrap()
            .run()
            .unwrap()
            .stop()
            .is_ok())
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
            capabilities!(
                AgentCapabilities::ReportsHealth,
                AgentCapabilities::AcceptsRemoteConfig
            ),
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
