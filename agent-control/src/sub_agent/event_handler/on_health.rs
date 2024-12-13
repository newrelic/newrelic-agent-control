use crate::agent_control::config::{AgentID, AgentTypeFQN};
use crate::event::channel::EventPublisher;
use crate::event::SubAgentEvent;
use crate::sub_agent::error::SubAgentError;
use crate::sub_agent::health::with_start_time::HealthWithStartTime;
use opamp_client::operation::callbacks::Callbacks;
use opamp_client::StartedClient;
use tracing::{debug, warn};

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
    if health.is_healthy() {
        debug!(select_arm = "sub_agent_internal_consumer", "HealthyAgent");
    } else {
        debug!(select_arm = "sub_agent_internal_consumer", "UnhealthyAgent");
        warn!(%agent_id, "sub agent became unhealthy!");
    }

    if let Some(client) = maybe_opamp_client.as_ref() {
        client.set_health(health.clone().into())?;
    }
    Ok(sub_agent_publisher.publish(SubAgentEvent::new(health, agent_id, agent_type))?)
}
