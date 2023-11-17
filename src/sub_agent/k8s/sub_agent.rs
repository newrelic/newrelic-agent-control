use crate::sub_agent::opamp::common::stop_opamp_client;
use crate::{
    config::super_agent_configs::AgentID,
    sub_agent::{error::SubAgentError, NotStartedSubAgent, StartedSubAgent},
};

////////////////////////////////////////////////////////////////////////////////////
// Not Started SubAgent On K8s
// C: OpAMP Client
////////////////////////////////////////////////////////////////////////////////////
pub struct NotStartedSubAgentK8s<C: opamp_client::StartedClient> {
    agent_id: AgentID,
    opamp_client: Option<C>,
    // TODO: store CRs supervisors
}

impl<C: opamp_client::StartedClient> NotStartedSubAgentK8s<C> {
    pub fn new(agent_id: AgentID, opamp_client: Option<C>) -> Self {
        NotStartedSubAgentK8s {
            agent_id,
            opamp_client,
        }
    }
}

impl<C: opamp_client::StartedClient> NotStartedSubAgent for NotStartedSubAgentK8s<C> {
    type StartedSubAgent = StartedSubAgentK8s<C>;

    fn run(self) -> Result<Self::StartedSubAgent, SubAgentError> {
        // TODO: handle stored CRs supervisors
        Ok(StartedSubAgentK8s::new(self.agent_id, self.opamp_client))
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
