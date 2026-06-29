//! Handles sub-agent health updates by reporting them to OpAMP and broadcasting a sub-agent event.

use crate::checkers::health::with_start_time::HealthWithStartTime;
use crate::event::SubAgentEvent;
use crate::event::broadcaster::unbounded::UnboundedBroadcast;
use crate::sub_agent::error::SubAgentError;
use crate::sub_agent::identity::AgentIdentity;
use opamp_client::StartedClient;

/// Reports the given health to the OpAMP client (if present) and broadcasts it as a [SubAgentEvent].
pub fn on_health<C>(
    health: HealthWithStartTime,
    maybe_opamp_client: Option<&C>,
    sub_agent_publisher: UnboundedBroadcast<SubAgentEvent>,
    agent_identity: AgentIdentity,
) -> Result<(), SubAgentError>
where
    C: StartedClient,
{
    if let Some(client) = maybe_opamp_client.as_ref() {
        client.set_health(health.clone().into())?;
    }
    sub_agent_publisher.broadcast(SubAgentEvent::new_health(agent_identity, health));
    Ok(())
}
