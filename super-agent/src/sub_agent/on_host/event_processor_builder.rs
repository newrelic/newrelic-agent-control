use crate::event::channel::{EventConsumer, EventPublisher};
use crate::event::{OpAMPEvent, SubAgentEvent};
#[cfg_attr(test, mockall_double::double)]
use crate::sub_agent::on_host::event_processor::EventProcessor;
use crate::sub_agent::SubAgentCallbacks;
use opamp_client::StartedClient;

pub struct SubAgentEventProcessorBuilder;

#[cfg_attr(test, mockall::automock)]
impl SubAgentEventProcessorBuilder {
    pub fn build<C>(
        &self,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
        sub_agent_opamp_consumer: EventConsumer<OpAMPEvent>,
        maybe_opamp_client: Option<C>,
    ) -> EventProcessor<C>
    where
        C: StartedClient<SubAgentCallbacks> + 'static,
    {
        EventProcessor::new(
            sub_agent_publisher,
            sub_agent_opamp_consumer,
            maybe_opamp_client,
        )
    }
}

#[cfg(test)]
pub mod test {
    use crate::sub_agent::on_host::event_processor::MockEventProcessor;
    use crate::sub_agent::on_host::event_processor_builder::MockSubAgentEventProcessorBuilder;
    use crate::sub_agent::SubAgentCallbacks;
    use opamp_client::StartedClient;

    impl MockSubAgentEventProcessorBuilder {
        pub fn should_build<C>(&mut self, processor: MockEventProcessor<C>)
        where
            C: StartedClient<SubAgentCallbacks> + 'static,
        {
            self.expect_build()
                .once()
                .return_once(move |_, _, _| processor);
        }
    }
}
