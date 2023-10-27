use crate::{
    command::stream::Event,
    config::{agent_type::agent_types::FinalAgent, super_agent_configs::AgentID},
    context::Context,
    opamp::client_builder::OpAMPClientBuilder,
    sub_agent::{
        error::{SubAgentBuilderError, SubAgentError},
        on_host::sub_agent_on_host::NotStartedSubAgentOnHost,
        SubAgentBuilder,
    },
    super_agent::instance_id::InstanceIDGetter,
    supervisor::{
        command_supervisor::NotStartedSupervisorOnHost,
        command_supervisor_config::SupervisorConfigOnHost, restart_policy::RestartPolicy,
    },
};

pub struct K8sSubAgentBuilder<'a, O, I>
where
    O: OpAMPClientBuilder,
    I: InstanceIDGetter,
{
    opamp_builder: Option<&'a O>,
    instance_id_getter: &'a I,
}

impl<'a, O, I> K8sSubAgentBuilder<'a, O, I>
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

impl<'a, O, I> SubAgentBuilder for K8sSubAgentBuilder<'a, O, I>
where
    O: OpAMPClientBuilder,
    I: InstanceIDGetter,
{
    type NotStartedSubAgent = NotStartedSubAgentOnHost<'a, O, I>;
    fn build(
        &self,
        agent: FinalAgent,
        tx: std::sync::mpsc::Sender<crate::command::stream::Event>,
    ) -> Result<Self::NotStartedSubAgent, SubAgentBuilderError> {
        let agent_type = agent.agent_type().clone();
        Ok(NotStartedSubAgentOnHost::new(
            AgentID("TODO".to_string()),
            build_supervisors(agent, tx)?,
            self.opamp_builder,
            self.instance_id_getter,
            agent_type,
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
