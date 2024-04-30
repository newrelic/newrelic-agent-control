use crate::event::SubAgentEvent::SubAgentBecameUnhealthy;
use crate::opamp::hash_repository::HashRepository;
use crate::sub_agent::error::SubAgentError;
use crate::sub_agent::event_processor::EventProcessor;
use crate::sub_agent::health::health_checker::Unhealthy;
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
    pub(crate) fn on_became_unhealthy(&self, unhealthy: Unhealthy) -> Result<(), SubAgentError> {
        if let Some(client) = self.maybe_opamp_client.as_ref() {
            let health = opamp_client::opamp::proto::ComponentHealth {
                healthy: false,
                start_time_unix_nano: get_sys_time_nano()?,
                last_error: unhealthy.last_error().to_string(),
                status: unhealthy.status().to_string(),
                ..Default::default()
            };
            client.set_health(health)?;
        }
        Ok(self
            .sub_agent_publisher
            .publish(SubAgentBecameUnhealthy(self.agent_id(), unhealthy))?)
    }
}
