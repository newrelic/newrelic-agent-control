use crate::event::SubAgentEvent;
use crate::opamp::effective_config::loader::EffectiveConfigLoader;
use crate::opamp::hash_repository::HashRepository;
use crate::sub_agent::error::SubAgentError;
use crate::sub_agent::event_processor::EventProcessor;
use crate::sub_agent::health::with_start_time::HealthWithStartTime;
use crate::sub_agent::SubAgentCallbacks;
use crate::values::yaml_config_repository::YAMLConfigRepository;
use opamp_client::StartedClient;

impl<C, H, Y, G> EventProcessor<C, H, Y, G>
where
    G: EffectiveConfigLoader,
    C: StartedClient<SubAgentCallbacks<G>> + 'static,
    H: HashRepository,
    Y: YAMLConfigRepository,
{
    pub(crate) fn on_health(&self, health: HealthWithStartTime) -> Result<(), SubAgentError> {
        if let Some(client) = self.maybe_opamp_client.as_ref() {
            client.set_health(health.clone().into())?;
        }
        Ok(self
            .sub_agent_publisher
            .publish(SubAgentEvent::new(health, self.agent_id()))?)
    }
}
