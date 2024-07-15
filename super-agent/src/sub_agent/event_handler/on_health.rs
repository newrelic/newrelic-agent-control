use crate::event::SubAgentEvent;
use crate::opamp::effective_config::loader::EffectiveConfigLoader;
use crate::opamp::hash_repository::HashRepository;
use crate::sub_agent::error::SubAgentError;
use crate::sub_agent::event_processor::EventProcessor;
use crate::sub_agent::health::with_start_time::HealthWithStartTime;
use crate::sub_agent::SubAgentCallbacks;
use crate::values::values_repository::ValuesRepository;
use opamp_client::StartedClient;
use tracing::error;

impl<C, H, R, G> EventProcessor<C, H, R, G>
where
    G: EffectiveConfigLoader,
    C: StartedClient<SubAgentCallbacks<G>> + 'static,
    H: HashRepository,
    R: ValuesRepository,
{
    pub(crate) fn on_health(&self, health: HealthWithStartTime) -> Result<(), SubAgentError> {
        if let Some(client) = self.maybe_opamp_client.as_ref() {
            let start_time_unix_nano = health
                .start_time()
                .duration_since(std::time::UNIX_EPOCH)
                .inspect_err(|e| {
                    error!("could not convert start time to unix timestamp nanoseconds: {e}")
                })?
                .as_nanos() as u64;
            let status_time_unix_nano = health
                .status_time()
                .duration_since(std::time::UNIX_EPOCH)
                .inspect_err(|e| {
                    error!("could not convert status time to unix timestamp nanoseconds: {e}")
                })?
                .as_nanos() as u64;
            let health = opamp_client::opamp::proto::ComponentHealth {
                healthy: health.is_healthy(),
                start_time_unix_nano,
                status_time_unix_nano,
                last_error: health.last_error().unwrap_or_default(),
                status: health.status().to_string(),
                ..Default::default()
            };
            client.set_health(health)?;
        }
        Ok(self
            .sub_agent_publisher
            .publish(SubAgentEvent::new(health, self.agent_id()))?)
    }
}
