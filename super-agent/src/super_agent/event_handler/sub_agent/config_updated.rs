use crate::event::channel::EventPublisher;
use crate::event::SubAgentEvent;
use crate::opamp::hash_repository::HashRepository;
use crate::sub_agent::collection::StartedSubAgents;
use crate::sub_agent::{NotStartedSubAgent, SubAgentBuilder};
use crate::super_agent::config::AgentID;
use crate::super_agent::error::AgentError;
use crate::super_agent::store::{
    SubAgentsConfigDeleter, SubAgentsConfigLoader, SubAgentsConfigStorer,
};
use crate::super_agent::{SuperAgent, SuperAgentCallbacks};
use opamp_client::StartedClient;

impl<S, O, HR, SL> SuperAgent<S, O, HR, SL>
where
    O: StartedClient<SuperAgentCallbacks>,
    HR: HashRepository,
    S: SubAgentBuilder,
    SL: SubAgentsConfigStorer + SubAgentsConfigLoader + SubAgentsConfigDeleter,
{
    pub(crate) fn sub_agent_config_updated(
        &self,
        agent_id: AgentID,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
        sub_agents: &mut StartedSubAgents<
            <S::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
    ) -> Result<(), AgentError> {
        let agents_config = self.sub_agents_config_store.load()?;
        let agent_config = agents_config.get(&agent_id)?;
        self.recreate_sub_agent(agent_id, agent_config, sub_agents, sub_agent_publisher)
    }
}
