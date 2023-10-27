use crate::command::stream::Event;
use crate::config::agent_type::agent_types::FinalAgent;
use crate::config::super_agent_configs::{AgentID, AgentTypeFQN};
use crate::context::Context;
use crate::opamp::client_builder::{OpAMPClientBuilder, OpAMPClientBuilderError};
use crate::sub_agent::on_host::sub_agent_on_host::NotStartedSubAgentOnHost;
use crate::sub_agent::on_host::sub_agents_on_host::NotStartedSubAgentsOnHost;
use crate::sub_agent::sub_agent::SubAgentError;
use crate::super_agent::instance_id::InstanceIDGetter;
use crate::super_agent::super_agent::{EffectiveAgents, SuperAgentEvent};
use crate::supervisor::command_supervisor::NotStartedSupervisorOnHost;
use crate::supervisor::command_supervisor_config::SupervisorConfigOnHost;
use crate::supervisor::restart_policy::RestartPolicy;
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
    effective_agents: EffectiveAgents,
    tx: &'a Sender<Event>,
    opamp_builder: Option<&'a OpAMPBuilder>,
    instance_id_getter: &'a ID,
) -> Result<NotStartedSubAgentsOnHost<'a, OpAMPBuilder, ID>, SubAgentError>
where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
{
    let mut sub_agents: NotStartedSubAgentsOnHost<'a, OpAMPBuilder, ID> =
        NotStartedSubAgentsOnHost::default();

    //TODO try to move this to a map
    let result: Result<(), SubAgentError> =
        effective_agents
            .agents
            .into_iter()
            .try_for_each(|(agent_id, final_agent)| {
                let builder = opamp_builder.as_ref().cloned();
                let sub_agent = build_sub_agent(
                    agent_id,
                    tx.clone(),
                    builder,
                    instance_id_getter,
                    final_agent,
                )?;
                sub_agents.add(sub_agent);
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
pub fn build_sub_agent<'a, OpAMPBuilder, ID>(
    agent_id: AgentID,
    tx: Sender<Event>,
    opamp_builder: Option<&'a OpAMPBuilder>,
    instance_id_getter: &'a ID,
    final_agent: FinalAgent,
) -> Result<NotStartedSubAgentOnHost<'a, OpAMPBuilder, ID>, SubAgentError>
where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
{
    let agent_type = final_agent.agent_type().clone();
    let supervisors = build_supervisors(final_agent, tx)?;
    Ok(NotStartedSubAgentOnHost::new(
        agent_id,
        supervisors,
        opamp_builder,
        instance_id_getter,
        agent_type,
    ))
}

fn build_supervisors(
    final_agent: FinalAgent,
    tx: Sender<Event>,
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

pub(super) fn build_opamp_and_start_client<OpAMPBuilder, InstanceIdGetter>(
    ctx: Context<Option<SuperAgentEvent>>,
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
            let start_settings =
                start_settings(instance_id_getter.get(agent_id.to_string()), agent_type);

            Ok(Some(builder.build_and_start(
                ctx,
                agent_id,
                start_settings,
            )?))
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
