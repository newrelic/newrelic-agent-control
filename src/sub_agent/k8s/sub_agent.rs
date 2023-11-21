use opamp_client::{operation::callbacks::Callbacks, StartedClient};

use crate::{
    config::super_agent_configs::AgentID,
    opamp::operations::stop_opamp_client,
    sub_agent::{error::SubAgentError, NotStartedSubAgent, StartedSubAgent},
};

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
    // TODO: store CRs supervisors

    // Needed to include this in the struct to avoid the compiler complaining about not using the type parameter `C`.
    // It's actually used as a generic parameter for the `OpAMPClientBuilder` instance bound by type parameter `O`.
    // Feel free to remove this when the actual implementations (Callbacks instance for K8s agents) make it redundant!
    _callbacks: std::marker::PhantomData<CB>,
    // supervisor: Supervisor<K8sExecutor>,
}

impl<CB, C> NotStartedSubAgentK8s<CB, C>
where
    CB: Callbacks,
    C: StartedClient<CB>,
{
    pub fn new(agent_id: AgentID, opamp_client: Option<C>) -> Self {
impl<C: opamp_client::StartedClient> NotStartedSubAgentK8s<C> {
    pub fn new(
        agent_id: AgentID,
        opamp_client: Option<C>,
        // supervisor: Supervisor<K8sExecutor>,
    ) -> Self {
        NotStartedSubAgentK8s {
            agent_id,
            opamp_client,

            _callbacks: std::marker::PhantomData,
            // supervisor: supervisor,
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
        // Start the supervisor
        // let _supervisor_handle = tokio::spawn(async move {
        //     self.supervisor
        //         .start()
        //         .await
        //         .expect("Failed to start supervisor");
        // });

        Ok(StartedSubAgentK8s::new(self.agent_id, self.opamp_client))
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Started SubAgent On K8s
// C: OpAMP Client
////////////////////////////////////////////////////////////////////////////////////
pub struct StartedSubAgentK8s<CB, C>
where
    CB: Callbacks,
    C: StartedClient<CB>,
{
    agent_id: AgentID,
    opamp_client: Option<C>,
    // TODO: CRs handle

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
    fn new(agent_id: AgentID, opamp_client: Option<C>) -> Self {
        StartedSubAgentK8s {
            agent_id,
            opamp_client,

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
        // TODO: stop CRs supervisors and return the corresponding JoinHandle
        Ok(vec![])
    }
}
