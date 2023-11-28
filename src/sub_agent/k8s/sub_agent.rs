use crate::sub_agent::k8s::CRSupervisor;
use crate::{
    config::super_agent_configs::AgentID,
    opamp::operations::stop_opamp_client,
    sub_agent::{error::SubAgentError, NotStartedSubAgent, StartedSubAgent},
};
use opamp_client::{operation::callbacks::Callbacks, StartedClient};

////////////////////////////////////////////////////////////////////////////////////
// Not Started SubAgent On K8s
// C: OpAMP Client
////////////////////////////////////////////////////////////////////////////////////
pub struct NotStartedSubAgentK8s<CB, C>
where
    CB: Callbacks,
    C: StartedClient<CB>,
{
    agent_id: AgentID,
    opamp_client: Option<C>,
    supervisor: CRSupervisor,

    // Needed to include this in the struct to avoid the compiler complaining about not using the type parameter `C`.
    // It's actually used as a generic parameter for the `OpAMPClientBuilder` instance bound by type parameter `O`.
    // Feel free to remove this when the actual implementations (Callbacks instance for K8s agents) make it redundant!
    _callbacks: std::marker::PhantomData<CB>,
}

impl<CB, C> NotStartedSubAgentK8s<CB, C>
where
    CB: Callbacks,
    C: StartedClient<CB>,
{
    pub fn new(agent_id: AgentID, opamp_client: Option<C>, supervisor: CRSupervisor) -> Self {
        NotStartedSubAgentK8s {
            agent_id,
            opamp_client,
            supervisor,
            _callbacks: std::marker::PhantomData,
        }
    }
}

impl<CB, C> NotStartedSubAgent for NotStartedSubAgentK8s<CB, C>
where
    CB: Callbacks,
    C: StartedClient<CB>,
{
    type StartedSubAgent = StartedSubAgentK8s<CB, C>;

    fn run(self) -> Result<Self::StartedSubAgent, SubAgentError> {
        self.supervisor.start().map_err(|e| {
            SubAgentError::SupervisorStopError(format!("Failed to start supervisor: {:?}", e))
        })?;

        Ok(StartedSubAgentK8s::new(
            self.agent_id,
            self.opamp_client,
            self.supervisor,
        ))
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Started SubAgent On K8s
// C: OpAMP Client
// S: Supervisor Trait
////////////////////////////////////////////////////////////////////////////////////
pub struct StartedSubAgentK8s<CB, C>
where
    CB: Callbacks,
    C: StartedClient<CB>,
{
    agent_id: AgentID,
    opamp_client: Option<C>,
    supervisor: CRSupervisor,

    // Needed to include this in the struct to avoid the compiler complaining about not using the type parameter `C`.
    // It's actually used as a generic parameter for the `OpAMPClientBuilder` instance bound by type parameter `O`.
    // Feel free to remove this when the actual implementations (Callbacks instance for K8s agents) make it redundant!
    _callbacks: std::marker::PhantomData<CB>,
}

impl<CB, C> StartedSubAgentK8s<CB, C>
where
    CB: Callbacks,
    C: StartedClient<CB>,
{
    fn new(agent_id: AgentID, opamp_client: Option<C>, supervisor: CRSupervisor) -> Self {
        StartedSubAgentK8s {
            agent_id,
            opamp_client,
            supervisor,
            _callbacks: std::marker::PhantomData,
        }
    }
}

impl<CB, C> StartedSubAgent for StartedSubAgentK8s<CB, C>
where
    CB: Callbacks,
    C: StartedClient<CB>,
{
    fn stop(self) -> Result<Vec<std::thread::JoinHandle<()>>, SubAgentError> {
        stop_opamp_client(self.opamp_client, &self.agent_id)?;

        self.supervisor.stop().map_err(|e| {
            SubAgentError::SupervisorStopError(format!("Failed to stop supervisor: {:?}", e))
        })?;

        Ok(vec![])
    }
}
