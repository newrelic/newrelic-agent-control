use crate::event::SubAgentEvent;
use crate::opamp::hash_repository::HashRepository;
use crate::sub_agent::error::SubAgentError;
use crate::sub_agent::event_processor::EventProcessor;
use crate::sub_agent::health::health_checker::Health;
use crate::sub_agent::values::values_repository::ValuesRepository;
use crate::sub_agent::SubAgentCallbacks;
use crate::utils::time::get_sys_time_nano;
use opamp_client::StartedClient;

impl<C, H, R> EventProcessor<C, H, R>
where
    C: StartedClient<SubAgentCallbacks> + 'static,
    H: HashRepository,
    R: ValuesRepository,
{
    pub(crate) fn on_health(&self, health: Health) -> Result<(), SubAgentError> {
        if let Some(client) = self.maybe_opamp_client.as_ref() {
            let health = opamp_client::opamp::proto::ComponentHealth {
                healthy: health.is_healthy(),
                status_time_unix_nano: get_sys_time_nano()?,
                last_error: health.last_error().unwrap_or("").to_string(),
                status: health.status().to_string(),
                ..Default::default()
            };
            client.set_health(health)?;
        }
        Ok(self
            .sub_agent_publisher
            .publish(SubAgentEvent::from_health(health, self.agent_id()))?)
    }
}
