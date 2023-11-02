use crate::{
    config::{
        agent_type::agent_types::FinalAgent, remote_config_hash::HashRepositoryFile,
        super_agent_configs::AgentID,
    },
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

use super::sub_agent::NotStartedSubAgentOnHost;
use super::supervisor::{
    command_supervisor::NotStartedSupervisorOnHost,
    command_supervisor_config::SupervisorConfigOnHost,
};

pub struct OnHostSubAgentBuilder<'a, O, I>
where
    O: OpAMPClientBuilder,
    I: InstanceIDGetter,
{
    opamp_builder: Option<&'a O>,
    instance_id_getter: &'a I,
}

impl<'a, O, I> OnHostSubAgentBuilder<'a, O, I>
where
    O: OpAMPClientBuilder,
    I: InstanceIDGetter,
{
    pub fn new(opamp_builder: Option<&'a O>, instance_id_getter: &'a I) -> Self {
        Self {
            opamp_builder,
            instance_id_getter,
        }
    }
}

impl<'a, O, I> SubAgentBuilder for OnHostSubAgentBuilder<'a, O, I>
where
    O: OpAMPClientBuilder,
    I: InstanceIDGetter,
{
    type NotStartedSubAgent = NotStartedSubAgentOnHost<'a, O, I>;
    fn build(
        &self,
        agent: FinalAgent,
        agent_id: AgentID,
        tx: std::sync::mpsc::Sender<Event>,
    ) -> Result<Self::NotStartedSubAgent, SubAgentBuilderError> {
        let agent_type = agent.agent_type().clone();
        Ok(NotStartedSubAgentOnHost::new(
            agent_id,
            build_supervisors(agent, tx)?,
            self.opamp_builder,
            self.instance_id_getter,
            agent_type,
            HashRepositoryFile::default(),
        ))
    }
    // add code here
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

    use crate::{
        config::agent_type::runtime_config::OnHost,
        opamp::client_builder::test::{MockOpAMPClientBuilderMock, MockOpAMPClientMock},
        super_agent::instance_id::test::MockInstanceIDGetterMock,
    };

    use super::*;

    use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent};

    #[test]
    fn build_start_stop() {
        let mut opamp_builder = MockOpAMPClientBuilderMock::new();
        let hostname = gethostname().unwrap_or_default().into_string().unwrap();
        let start_settings_infra = infra_agent_default_start_settings(&hostname);

        // Infra Agent OpAMP no final stop nor health, just after stopping on reload
        opamp_builder.should_build_and_start(
            AgentID::new("infra_agent"),
            start_settings_infra,
            |_, _, _| {
                let mut started_client = MockOpAMPClientMock::new();
                started_client.should_set_health(1);
                started_client.should_stop(1);
                Ok(started_client)
            },
        );

        let mut instance_id_getter = MockInstanceIDGetterMock::new();
        instance_id_getter.should_get(
            "infra_agent".to_string(),
            "infra_agent_instance_id".to_string(),
        );

        let on_host_builder = OnHostSubAgentBuilder::new(Some(&opamp_builder), &instance_id_getter);

        let (tx, _rx) = channel();

        assert!(on_host_builder
            .build(on_host_final_agent(), AgentID::new("infra_agent"), tx)
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
