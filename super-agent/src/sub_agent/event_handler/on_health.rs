use crate::event::SubAgentEvent;
use crate::opamp::hash_repository::HashRepository;
use crate::sub_agent::error::SubAgentError;
use crate::sub_agent::event_processor::EventProcessor;
use crate::sub_agent::health::with_start_time::HealthWithTimes;
use crate::sub_agent::values::values_repository::ValuesRepository;
use crate::sub_agent::SubAgentCallbacks;
use opamp_client::StartedClient;
use tracing::error;

impl<C, H, R> EventProcessor<C, H, R>
where
    C: StartedClient<SubAgentCallbacks> + 'static,
    H: HashRepository,
    R: ValuesRepository,
{
    pub(crate) fn on_health(&self, health: HealthWithTimes) -> Result<(), SubAgentError> {
        if let Some(client) = self.maybe_opamp_client.as_ref() {
            let start_time_unix_nano = health
                .start_time()
                .duration_since(std::time::UNIX_EPOCH)
                .inspect_err(|e| {
                    error!("could not convert stored start time to unix timestamp nanoseconds")
                })?
                .as_nanos() as u64;
            let status_time_unix_nano = health
                .status_time()
                .duration_since(std::time::UNIX_EPOCH)
                .inspect_err(|e| {
                    error!("could not convert stored status time to unix timestamp nanoseconds")
                })?
                .as_nanos() as u64;
            let health = opamp_client::opamp::proto::ComponentHealth {
                healthy: health.is_healthy(),
                start_time_unix_nano,
                status_time_unix_nano,
                last_error: health.last_error().unwrap_or("").to_string(),
                status: health.status().to_string(),
                ..Default::default()
            };
            client.set_health(health)?;
        }
        Ok(self
            .sub_agent_publisher
            .publish(SubAgentEvent::from_health_with_times(
                health,
                self.agent_id(),
            ))?)
    }
}
