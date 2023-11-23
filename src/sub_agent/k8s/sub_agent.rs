use crate::sub_agent::k8s::supervisor::SupervisorTrait;
use opamp_client::{operation::callbacks::Callbacks, StartedClient};

use crate::k8s::executor::K8sDynamicObjectsManager;
use crate::sub_agent::k8s::supervisor::Supervisor;
use crate::sub_agent::opamp::common::stop_opamp_client;
use crate::{
    config::super_agent_configs::AgentID,
    opamp::operations::stop_opamp_client,
    sub_agent::{error::SubAgentError, NotStartedSubAgent, StartedSubAgent},
};

////////////////////////////////////////////////////////////////////////////////////
// Not Started SubAgent On K8s
// S: Supervisor Trait
////////////////////////////////////////////////////////////////////////////////////
pub struct NotStartedSubAgentK8s<CB, C>
where
    CB: Callbacks,
    C: StartedClient<CB>,
{
pub struct NotStartedSubAgentK8s<C, E>
pub struct NotStartedSubAgentK8s<C, S>
where
    C: opamp_client::StartedClient,
    S: SupervisorTrait,
{
    agent_id: AgentID,
    opamp_client: Option<C>,
    supervisor: S,
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
    supervisor: Supervisor<E>,
}

impl<C, S> NotStartedSubAgentK8s<C, S>
where
    C: opamp_client::StartedClient,
    S: SupervisorTrait,
{
    pub fn new(agent_id: AgentID, opamp_client: Option<C>, supervisor: S) -> Self {
        NotStartedSubAgentK8s {
            agent_id,
            opamp_client,

            _callbacks: std::marker::PhantomData,
            // supervisor: supervisor,
            supervisor,
        }
    }
}

impl<CB, C> NotStartedSubAgent for NotStartedSubAgentK8s<CB, C>
where
    CB: Callbacks,
    C: StartedClient<CB>,
{
    type StartedSubAgent = StartedSubAgentK8s<CB, C>;
impl<C, E> NotStartedSubAgent for NotStartedSubAgentK8s<C, E>
impl<C, S> NotStartedSubAgent for NotStartedSubAgentK8s<C, S>
where
    C: opamp_client::StartedClient,
    S: SupervisorTrait,
{
    type StartedSubAgent = StartedSubAgentK8s<C, S>;

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
pub struct StartedSubAgentK8s<C, E>
pub struct StartedSubAgentK8s<C, S>
where
    C: opamp_client::StartedClient,
    S: SupervisorTrait,
{
    agent_id: AgentID,
    opamp_client: Option<C>,
    // TODO: CRs handle

    // Needed to include this in the struct to avoid the compiler complaining about not using the type parameter `C`.
    // It's actually used as a generic parameter for the `OpAMPClientBuilder` instance bound by type parameter `O`.
    // Feel free to remove this when the actual implementations (Callbacks instance for K8s agents) make it redundant!
    _callbacks: std::marker::PhantomData<CB>,
    supervisor: Supervisor<E>,
    supervisor: S,
}

impl<CB, C> StartedSubAgentK8s<CB, C>
where
    CB: Callbacks,
    C: StartedClient<CB>,
{
    fn new(agent_id: AgentID, opamp_client: Option<C>) -> Self {
impl<C, E> StartedSubAgentK8s<C, E>
impl<C, S> StartedSubAgentK8s<C, S>
where
    C: opamp_client::StartedClient,
    S: SupervisorTrait,
{
    fn new(agent_id: AgentID, opamp_client: Option<C>, supervisor: S) -> Self {
        StartedSubAgentK8s {
            agent_id,
            opamp_client,

            _callbacks: std::marker::PhantomData,
            supervisor,
        }
    }
}

impl<CB, C> StartedSubAgent for StartedSubAgentK8s<CB, C>
where
    CB: Callbacks,
    C: StartedClient<CB>,
{
impl<C, E> StartedSubAgent for StartedSubAgentK8s<C, E>
impl<C, S> StartedSubAgent for StartedSubAgentK8s<C, S>
where
    C: opamp_client::StartedClient,
    S: SupervisorTrait,
{
    fn stop(self) -> Result<Vec<std::thread::JoinHandle<()>>, SubAgentError> {
        stop_opamp_client(self.opamp_client, &self.agent_id)?;

        self.supervisor.stop().map_err(|e| {
            SubAgentError::SupervisorStopError(format!("Failed to stop supervisor: {:?}", e))
        })?;

        Ok(vec![])
    }
}
