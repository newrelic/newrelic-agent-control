use std::collections::HashMap;

use crate::sub_agent::opamp::common::{start_opamp_client, start_settings, stop_opamp_client};
use crate::super_agent::super_agent::SuperAgentEvent;
use crate::{
    config::super_agent_configs::{AgentID, AgentTypeFQN},
    context::Context,
    opamp::client_builder::{OpAMPClientBuilder, OpAMPClientBuilderError},
    sub_agent::{error::SubAgentError, NotStartedSubAgent, StartedSubAgent},
    super_agent::instance_id::InstanceIDGetter,
};

////////////////////////////////////////////////////////////////////////////////////
// Not Started SubAgent On K8s
// C: OpAMP Client
////////////////////////////////////////////////////////////////////////////////////
pub struct NotStartedSubAgentK8s<'a, O, I>
where
    O: OpAMPClientBuilder,
    I: InstanceIDGetter,
{
    agent_id: AgentID,
    agent_type: AgentTypeFQN,
    opamp_builder: Option<&'a O>,
    instance_id_getter: &'a I,
    ctx: Context<Option<SuperAgentEvent>>,
    // TODO: store CRs supervisors
}

impl<'a, O, I> NotStartedSubAgentK8s<'a, O, I>
where
    O: OpAMPClientBuilder,
    I: InstanceIDGetter,
{
    pub fn new(
        agent_id: AgentID,
        agent_type: AgentTypeFQN,
        opamp_builder: Option<&'a O>,
        instance_id_getter: &'a I,
        ctx: Context<Option<SuperAgentEvent>>,
    ) -> Self {
        NotStartedSubAgentK8s {
            agent_id,
            agent_type,
            opamp_builder,
            instance_id_getter,
            ctx,
        }
    }
}

impl<'a, O, I> NotStartedSubAgentK8s<'a, O, I>
where
    O: OpAMPClientBuilder,
    I: InstanceIDGetter,
{
    fn start_opamp_client(&self) -> Result<Option<O::Client>, OpAMPClientBuilderError> {
        let opamp_start_settings = start_settings(
            self.instance_id_getter.get(&self.agent_id),
            &self.agent_type,
            HashMap::new(), // TODO: check if some non-identifying attributes are needed
        );
        start_opamp_client(
            self.ctx.clone(),
            self.opamp_builder,
            self.agent_id.clone(),
            opamp_start_settings,
        )
    }
}

impl<'a, O, I> NotStartedSubAgent for NotStartedSubAgentK8s<'a, O, I>
where
    O: OpAMPClientBuilder,
    I: InstanceIDGetter,
{
    type StartedSubAgent = StartedSubAgentK8s<O::Client>;

    fn run(self) -> Result<Self::StartedSubAgent, SubAgentError> {
        let opamp_client = self.start_opamp_client()?;
        // TODO: handle stored CRs supervisors
        Ok(StartedSubAgentK8s::new(self.agent_id, opamp_client))
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Started SubAgent On K8s
// C: OpAMP Client
////////////////////////////////////////////////////////////////////////////////////
pub struct StartedSubAgentK8s<C: opamp_client::StartedClient> {
    agent_id: AgentID,
    opamp_client: Option<C>,
    // TODO: CRs handle
}

impl<C: opamp_client::StartedClient> StartedSubAgentK8s<C> {
    fn new(agent_id: AgentID, opamp_client: Option<C>) -> Self {
        StartedSubAgentK8s {
            agent_id,
            opamp_client,
        }
    }
}

impl<C: opamp_client::StartedClient> StartedSubAgent for StartedSubAgentK8s<C> {
    fn stop(self) -> Result<Vec<std::thread::JoinHandle<()>>, SubAgentError> {
        stop_opamp_client(self.opamp_client, &self.agent_id)?;
        // TODO: stop CRs supervisors and return the corresponding JoinHandle
        Ok(vec![])
    }
}
