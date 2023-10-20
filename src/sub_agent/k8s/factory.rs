use crate::command::stream::Event;
use crate::config::agent_configs::{AgentID, AgentTypeFQN};
use crate::config::agent_type::agent_types::FinalAgent;
use crate::opamp::client_builder::{OpAMPClientBuilder, OpAMPClientBuilderError};
use crate::sub_agent::k8s::sub_agent_k8s::NotStartedSubAgentK8S;
use crate::sub_agent::sub_agent_strategy::NotStartedSubAgentStrategy;
use crate::sub_agent::sub_agent::SubAgentError;
use crate::sub_agent::sub_agents::StaticNotStartedSubAgents;
use crate::super_agent::instance_id::InstanceIDGetter;
use crate::super_agent::super_agent::EffectiveAgents;
use nix::unistd::gethostname;
use opamp_client::capabilities;
use opamp_client::opamp::proto::AgentCapabilities;
use opamp_client::operation::settings::{AgentDescription, StartSettings};
use std::collections::HashMap;
use std::sync::mpsc::Sender;

////////////////////////////////////////////////////////////////////////////////////
// Build SubAgents On Host
////////////////////////////////////////////////////////////////////////////////////

pub fn build_sub_agents<'a, OpAMPBuilder, ID>(
    effective_agents: &EffectiveAgents,
    tx: Sender<Event>,
    opamp_builder: Option<&'a OpAMPBuilder>,
    instance_id_getter: &'a ID,
) -> Result<StaticNotStartedSubAgents<'a, OpAMPBuilder, ID>, SubAgentError>
where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
{
    let mut sub_agents: StaticNotStartedSubAgents<'a, OpAMPBuilder, ID> =
        StaticNotStartedSubAgents::default();

    //TODO try to move this to a map
    let result: Result<(), SubAgentError> =
        effective_agents
            .agents
            .iter()
            .try_for_each(|(agent_id, final_agent)| {
                // TODO : remove RC by &
                let builder = opamp_builder.as_ref().cloned();
                let agent_id = AgentID(agent_id.to_string());
                let sub_agent = build_sub_agent(
                    agent_id.clone(),
                    &tx,
                    builder,
                    instance_id_getter,
                    final_agent,
                )?;
                sub_agents.add(agent_id.clone(), NotStartedSubAgentStrategy::K8S(sub_agent));
                Ok(())
            });
    match result {
        Err(e) => Err(e),
        _ => Ok(sub_agents),
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Build SubAgent On Host
////////////////////////////////////////////////////////////////////////////////////
pub(super) fn build_sub_agent<'a, OpAMPBuilder, ID>(
    agent_id: AgentID,
    _tx: &Sender<Event>,
    opamp_builder: Option<&'a OpAMPBuilder>,
    instance_id_getter: &'a ID,
    final_agent: &FinalAgent,
) -> Result<NotStartedSubAgentK8S<'a, OpAMPBuilder, ID>, SubAgentError>
where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
{
    Ok(NotStartedSubAgentK8S::new(
        agent_id.clone(),
        opamp_builder,
        instance_id_getter,
        final_agent.agent_type(),
    ))
}

pub(super) fn build_opamp_and_start_client<OpAMPBuilder, InstanceIdGetter>(
    opamp_builder: Option<&OpAMPBuilder>,
    instance_id_getter: &InstanceIdGetter,
    agent_id: AgentID,
    agent_type: &AgentTypeFQN,
) -> Result<Option<OpAMPBuilder::Client>, OpAMPClientBuilderError>
where
    OpAMPBuilder: OpAMPClientBuilder,
    InstanceIdGetter: InstanceIDGetter,
{
    match opamp_builder {
        Some(builder) => {
            let start_settings = start_settings(instance_id_getter.get(agent_id.get()), agent_type);

            println!("{:?}", start_settings);
            Ok(Some(builder.build_and_start(start_settings)?))
        }
        None => Ok(None),
    }
}

fn start_settings(instance_id: String, agent_fqn: &AgentTypeFQN) -> StartSettings {
    StartSettings {
        instance_id,
        capabilities: capabilities!(AgentCapabilities::ReportsHealth),
        agent_description: AgentDescription {
            identifying_attributes: HashMap::from([
                ("service.name".to_string(), agent_fqn.name().into()),
                (
                    "service.namespace".to_string(),
                    agent_fqn.namespace().into(),
                ),
                ("service.version".to_string(), agent_fqn.version().into()),
            ]),
            non_identifying_attributes: HashMap::from([(
                "host.name".to_string(),
                get_hostname().into(),
            )]),
        },
    }
}

fn get_hostname() -> String {
    #[cfg(unix)]
    return gethostname().unwrap_or_default().into_string().unwrap();

    #[cfg(not(unix))]
    return unimplemented!();
}
