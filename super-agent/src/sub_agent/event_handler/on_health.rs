use crate::event::channel::EventPublisher;
use crate::event::SubAgentEvent;
use crate::sub_agent::error::SubAgentError;
use crate::sub_agent::health::with_start_time::HealthWithStartTime;
use crate::super_agent::config::{AgentID, AgentTypeFQN};
use opamp_client::operation::callbacks::Callbacks;
use opamp_client::StartedClient;

pub fn on_health<C, CB>(
    health: HealthWithStartTime,
    maybe_opamp_client: Option<&C>,
    sub_agent_publisher: EventPublisher<SubAgentEvent>,
    agent_id: AgentID,
    agent_type: AgentTypeFQN,
) -> Result<(), SubAgentError>
where
    C: StartedClient<CB>,
    CB: Callbacks,
{
    if let Some(client) = maybe_opamp_client.as_ref() {
        client.set_health(health.clone().into())?;
    }
    Ok(sub_agent_publisher.publish(SubAgentEvent::new(health, agent_id, agent_type))?)
}
