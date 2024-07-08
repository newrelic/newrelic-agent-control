use crate::event::channel::EventPublisher;
use crate::event::SubAgentEvent;
use crate::opamp::hash_repository::HashRepository;
use crate::sub_agent::collection::StartedSubAgents;
use crate::sub_agent::values::values_repository::ValuesRepository;
use crate::sub_agent::{NotStartedSubAgent, SubAgentBuilder};
use crate::super_agent::config::{AgentID, SuperAgentConfigError};
use crate::super_agent::config_storer::loader_storer::{
    SuperAgentDynamicConfigDeleter, SuperAgentDynamicConfigLoader, SuperAgentDynamicConfigStorer,
};
use crate::super_agent::error::AgentError;
use crate::super_agent::{SuperAgent, SuperAgentCallbacks};
use opamp_client::StartedClient;

impl<S, O, HR, SL, R> SuperAgent<S, O, HR, SL, R>
where
    R: ValuesRepository,
    O: StartedClient<SuperAgentCallbacks<R>>,
    HR: HashRepository,
    S: SubAgentBuilder,
    SL: SuperAgentDynamicConfigStorer
        + SuperAgentDynamicConfigLoader
        + SuperAgentDynamicConfigDeleter,
{
    pub(crate) fn sub_agent_config_updated(
        &self,
        agent_id: AgentID,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
        sub_agents: &mut StartedSubAgents<
            <S::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
    ) -> Result<(), AgentError> {
        let super_agent_dynamic_config = self.sa_dynamic_config_store.load()?;
        let agent_config = super_agent_dynamic_config.agents.get(&agent_id).ok_or(
            SuperAgentConfigError::SubAgentNotFound(agent_id.to_string()),
        )?;
        self.recreate_sub_agent(agent_id, agent_config, sub_agents, sub_agent_publisher)
    }
}
