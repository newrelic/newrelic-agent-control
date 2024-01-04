use crate::config::store::{SubAgentsConfigDeleter, SubAgentsConfigLoader, SubAgentsConfigStorer};
use crate::config::super_agent_configs::AgentID;
use crate::event::channel::EventPublisher;
use crate::event::SubAgentEvent;
use crate::opamp::remote_config_hash::HashRepository;
use crate::sub_agent::collection::StartedSubAgents;
use crate::sub_agent::logger::AgentLog;
use crate::sub_agent::{NotStartedSubAgent, SubAgentBuilder};
use crate::super_agent::error::AgentError;
use crate::super_agent::super_agent::{SuperAgent, SuperAgentCallbacks};
use opamp_client::StartedClient;
use std::sync::mpsc::Sender;

impl<'a, S, O, HR, SL> SuperAgent<'a, S, O, HR, SL>
where
    O: StartedClient<SuperAgentCallbacks>,
    HR: HashRepository,
    S: SubAgentBuilder,
    SL: SubAgentsConfigStorer + SubAgentsConfigLoader + SubAgentsConfigDeleter,
{
    pub(crate) fn sub_agent_config_updated(
        &self,
        agent_id: AgentID,
        tx: Sender<AgentLog>,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
        sub_agents: &mut StartedSubAgents<
            <S::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
    ) -> Result<(), AgentError> {
        let agents_config = self.sub_agents_config_store.load()?;
        let agent_config = agents_config.get(&agent_id)?;
        self.recreate_sub_agent(
            agent_id,
            agent_config,
            tx.clone(),
            sub_agents,
            sub_agent_publisher,
        )
    }
}
