use std::marker::PhantomData;
use std::thread::JoinHandle;

use opamp_client;
use opamp_client::StartedClient;
use tracing::debug;

use super::supervisor::command_supervisor::{NotStartedSupervisorOnHost, StartedSupervisorOnHost};
use crate::config::super_agent_configs::AgentID;
use crate::event::event::Event;
use crate::event::EventPublisher;
use crate::opamp::operations::stop_opamp_client;
use crate::sub_agent::error::SubAgentError;

use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent, SubAgentCallbacks};

////////////////////////////////////////////////////////////////////////////////////
// Not Started SubAgent On Host
// C: OpAMP Client
////////////////////////////////////////////////////////////////////////////////////
pub struct NotStartedSubAgentOnHost<C, P>
where
    C: StartedClient<SubAgentCallbacks<P>>,
    P: EventPublisher<Event> + Sync + Send,
{
    opamp_client: Option<C>,
    supervisors: Vec<NotStartedSupervisorOnHost>,
    agent_id: AgentID,
    pmarker: PhantomData<P>,
}

impl<C, P> NotStartedSubAgentOnHost<C, P>
where
    C: StartedClient<SubAgentCallbacks<P>>,
    P: EventPublisher<Event> + Sync + Send,
{
    pub fn new(
        agent_id: AgentID,
        supervisors: Vec<NotStartedSupervisorOnHost>,
        opamp_client: Option<C>,
    ) -> Result<Self, SubAgentError> {
        Ok(NotStartedSubAgentOnHost {
            opamp_client,
            supervisors,
            agent_id,
            pmarker: PhantomData,
        })
    }

    pub fn agent_id(&self) -> &AgentID {
        &self.agent_id
    }
}

impl<C, P> NotStartedSubAgent for NotStartedSubAgentOnHost<C, P>
where
    C: StartedClient<SubAgentCallbacks<P>>,
    P: EventPublisher<Event> + Sync + Send,
{
    type StartedSubAgent = StartedSubAgentOnHost<C, P>;

    fn run(self) -> Result<Self::StartedSubAgent, SubAgentError> {
        let started_supervisors = self
            .supervisors
            .into_iter()
            .map(|s| {
                debug!("Running supervisor {} for {}", s.config.bin, self.agent_id);
                s.run()
            })
            .collect::<Result<Vec<_>, _>>()?;

        let started_sub_agent = StartedSubAgentOnHost {
            opamp_client: self.opamp_client,
            supervisors: started_supervisors,
            agent_id: self.agent_id,
            pmarker: PhantomData,
        };

        Ok(started_sub_agent)
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Started SubAgent On Host
// C: OpAMP Client
////////////////////////////////////////////////////////////////////////////////////
pub struct StartedSubAgentOnHost<C, P>
where
    C: StartedClient<SubAgentCallbacks<P>>,
    P: EventPublisher<Event> + Sync + Send,
{
    opamp_client: Option<C>,
    supervisors: Vec<StartedSupervisorOnHost>,
    agent_id: AgentID,
    pmarker: PhantomData<P>,
}

impl<C, P> StartedSubAgent for StartedSubAgentOnHost<C, P>
where
    C: StartedClient<SubAgentCallbacks<P>>,
    P: EventPublisher<Event> + Sync + Send,
{
    fn stop(self) -> Result<Vec<JoinHandle<()>>, SubAgentError> {
        let stopped_supervisors = self.supervisors.into_iter().map(|s| s.stop()).collect();
        stop_opamp_client(self.opamp_client, &self.agent_id)?;
        Ok(stopped_supervisors)
    }
}
