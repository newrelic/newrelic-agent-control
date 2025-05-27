use crate::event::SubAgentEvent;
use crate::event::channel::EventPublisher;
use crate::sub_agent::error::SubAgentError;
use crate::sub_agent::health::with_start_time::HealthWithStartTime;
use crate::sub_agent::identity::AgentIdentity;
use opamp_client::StartedClient;

pub fn on_health<C>(
    health: HealthWithStartTime,
    maybe_opamp_client: Option<&C>,
    sub_agent_publisher: Option<EventPublisher<SubAgentEvent>>,
    agent_identity: AgentIdentity,
) -> Result<(), SubAgentError>
where
    C: StartedClient,
{
    if let Some(client) = maybe_opamp_client.as_ref() {
        client.set_health(health.clone().into())?;
    }
    sub_agent_publisher
        .as_ref()
        .map(|c| c.publish(SubAgentEvent::new_health(agent_identity, health)))
        .transpose()?;
    Ok(())
}
