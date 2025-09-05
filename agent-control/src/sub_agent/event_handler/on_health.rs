use crate::event::SubAgentEvent;
use crate::event::broadcaster::unbounded::UnboundedBroadcast;
use crate::health::with_start_time::HealthWithStartTime;
use crate::sub_agent::error::SubAgentError;
use crate::sub_agent::identity::SubAgentIdentity;
use opamp_client::StartedClient;

pub fn on_health<C>(
    health: HealthWithStartTime,
    maybe_opamp_client: Option<&C>,
    sub_agent_publisher: UnboundedBroadcast<SubAgentEvent>,
    agent_identity: SubAgentIdentity,
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
