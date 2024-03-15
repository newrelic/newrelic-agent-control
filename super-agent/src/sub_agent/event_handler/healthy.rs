use crate::event::SubAgentEvent::SubAgentHealthy;
use crate::opamp::hash_repository::HashRepository;
use crate::sub_agent::error::SubAgentError;
use crate::sub_agent::event_processor::EventProcessor;
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
    pub(crate) fn healthy(&self) -> Result<(), SubAgentError> {
        if let Some(client) = self.maybe_opamp_client.as_ref() {
            let health = opamp_client::opamp::proto::ComponentHealth {
                healthy: true,
                start_time_unix_nano: get_sys_time_nano()?,
                last_error: "".to_string(),
                ..Default::default()
            };
            client.set_health(health)?;
            self.sub_agent_publisher
                .publish(SubAgentHealthy(self.agent_id()))?;
            Ok(())
        } else {
            unreachable!()
        }
    }
}
