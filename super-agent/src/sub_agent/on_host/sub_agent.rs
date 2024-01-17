use std::marker::PhantomData;
use std::thread::JoinHandle;

use opamp_client;
use opamp_client::StartedClient;
use tracing::debug;

use super::supervisor::command_supervisor::SupervisorOnHost;
use crate::config::super_agent_configs::AgentID;
use crate::opamp::operations::stop_opamp_client;
use crate::sub_agent::error::SubAgentError;

use super::supervisor::command_supervisor;
use crate::sub_agent::on_host::event_processor::SubAgentEventProcessor;
use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent, SubAgentCallbacks};

////////////////////////////////////////////////////////////////////////////////////
// States for Started/Not Started Sub Agents
////////////////////////////////////////////////////////////////////////////////////
pub struct NotStarted<C, E>
where
    C: StartedClient<SubAgentCallbacks> + 'static,
    E: SubAgentEventProcessor<C>,
{
    event_processor: E,
    _marker: PhantomData<C>,
}

pub struct Started<C>
where
    C: StartedClient<SubAgentCallbacks>,
{
    event_loop_handle: JoinHandle<Option<C>>,
}

////////////////////////////////////////////////////////////////////////////////////
// Not Started SubAgent On Host
// C: OpAMP Client
////////////////////////////////////////////////////////////////////////////////////
pub struct SubAgentOnHost<S, V> {
    supervisors: Vec<SupervisorOnHost<V>>,
    agent_id: AgentID,
    state: S,
}

impl<S, V> SubAgentOnHost<S, V> {
    pub fn agent_id(&self) -> &AgentID {
        &self.agent_id
    }
}

impl<C, E> SubAgentOnHost<NotStarted<C, E>, command_supervisor::NotStarted>
where
    C: StartedClient<SubAgentCallbacks> + 'static,
    E: SubAgentEventProcessor<C>,
{
    pub fn new(
        agent_id: AgentID,
        supervisors: Vec<SupervisorOnHost<command_supervisor::NotStarted>>,
        event_processor: E,
    ) -> SubAgentOnHost<NotStarted<C, E>, command_supervisor::NotStarted> {
        SubAgentOnHost {
            supervisors,
            agent_id,
            state: NotStarted {
                event_processor,
                _marker: PhantomData,
            },
        }
    }
}

impl<C, E> NotStartedSubAgent for SubAgentOnHost<NotStarted<C, E>, command_supervisor::NotStarted>
where
    C: StartedClient<SubAgentCallbacks> + 'static,
    E: SubAgentEventProcessor<C>,
{
    type StartedSubAgent = SubAgentOnHost<Started<C>, command_supervisor::Started>;

    fn run(self) -> Result<SubAgentOnHost<Started<C>, command_supervisor::Started>, SubAgentError> {
        let started_supervisors = self
            .supervisors
            .into_iter()
            .map(|s| {
                debug!("Running supervisor {} for {}", s.id(), self.agent_id);
                s.run()
            })
            .collect::<Result<Vec<_>, _>>()?;

        let started_sub_agent = SubAgentOnHost {
            supervisors: started_supervisors,
            agent_id: self.agent_id,
            state: Started {
                event_loop_handle: self.state.event_processor.process(),
            },
        };

        Ok(started_sub_agent)
    }
}

impl<C> StartedSubAgent for SubAgentOnHost<Started<C>, command_supervisor::Started>
where
    C: StartedClient<SubAgentCallbacks>,
{
    fn stop(self) -> Result<Vec<JoinHandle<()>>, SubAgentError> {
        let stopped_supervisors = self.supervisors.into_iter().map(|s| s.stop()).collect();
        let maybe_opamp_client = self.state.event_loop_handle.join().unwrap();
        stop_opamp_client(maybe_opamp_client, &self.agent_id)?;
        Ok(stopped_supervisors)
    }
}

#[cfg(test)]
mod test {
    use crate::config::super_agent_configs::AgentID;
    use crate::opamp::client_builder::test::MockStartedOpAMPClientMock;
    use crate::sub_agent::on_host::event_processor::test::MockEventProcessorMock;
    use crate::sub_agent::on_host::sub_agent::SubAgentOnHost;
    use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent, SubAgentCallbacks};
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_events_are_processed() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let supervisors = Vec::default();

        let mut opamp_client: MockStartedOpAMPClientMock<SubAgentCallbacks> =
            MockStartedOpAMPClientMock::new();

        let mut event_processor = MockEventProcessorMock::default();

        opamp_client.should_set_health(1);
        opamp_client.should_stop(1);

        event_processor.should_process(Some(opamp_client));

        let sub_agent = SubAgentOnHost::new(agent_id, supervisors, event_processor);

        let started_agent = sub_agent.run().unwrap();
        sleep(Duration::from_millis(20));
        // close the OpAMP Publisher

        let handles = started_agent.stop().unwrap();

        for handle in handles {
            handle.join().unwrap();
        }
        println!("END")
    }
}
