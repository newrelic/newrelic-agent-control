use crate::event::channel::{EventConsumer, EventPublisher};
use crate::event::{OpAMPEvent, SubAgentEvent, SubAgentInternalEvent};
use crate::opamp::hash_repository::HashRepository;
use crate::sub_agent::event_processor::{EventProcessor, SubAgentEventProcessor};
use crate::sub_agent::values::values_repository::ValuesRepository;
use crate::sub_agent::SubAgentCallbacks;
use crate::super_agent::config::AgentID;
use opamp_client::StartedClient;
use std::sync::Arc;

pub trait SubAgentEventProcessorBuilder<C>
where
    C: StartedClient<SubAgentCallbacks> + 'static,
{
    type SubAgentEventProcessor: SubAgentEventProcessor;

    fn build(
        &self,
        agent_id: AgentID,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
        sub_agent_opamp_consumer: EventConsumer<OpAMPEvent>,
        sub_agent_internal_consumer: EventConsumer<SubAgentInternalEvent>,
        maybe_opamp_client: Option<C>,
    ) -> Self::SubAgentEventProcessor;
}

pub struct EventProcessorBuilder<H, R>
where
    H: HashRepository,
    R: ValuesRepository,
{
    hash_repository: Arc<H>,
    values_repository: Arc<R>,
}

impl<H, R> EventProcessorBuilder<H, R>
where
    H: HashRepository,
    R: ValuesRepository,
{
    pub fn new(hash_repository: Arc<H>, values_repository: Arc<R>) -> EventProcessorBuilder<H, R> {
        EventProcessorBuilder {
            values_repository,
            hash_repository,
        }
    }
}

impl<C, H, R> SubAgentEventProcessorBuilder<C> for EventProcessorBuilder<H, R>
where
    C: StartedClient<SubAgentCallbacks> + 'static,
    H: HashRepository + Send + Sync + 'static,
    R: ValuesRepository + Send + Sync + 'static,
{
    type SubAgentEventProcessor = EventProcessor<C, H, R>;

    fn build(
        &self,
        agent_id: AgentID,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
        sub_agent_opamp_consumer: EventConsumer<OpAMPEvent>,
        sub_agent_internal_consumer: EventConsumer<SubAgentInternalEvent>,
        maybe_opamp_client: Option<C>,
    ) -> EventProcessor<C, H, R>
    where
        C: StartedClient<SubAgentCallbacks> + 'static,
    {
        EventProcessor::new(
            agent_id,
            sub_agent_publisher,
            sub_agent_opamp_consumer,
            sub_agent_internal_consumer,
            maybe_opamp_client,
            self.hash_repository.clone(),
            self.values_repository.clone(),
        )
    }
}

#[cfg(test)]
pub mod test {
    use crate::event::channel::{EventConsumer, EventPublisher};
    use crate::event::{OpAMPEvent, SubAgentEvent, SubAgentInternalEvent};
    use crate::sub_agent::event_processor::test::MockEventProcessorMock;
    use crate::sub_agent::event_processor_builder::SubAgentEventProcessorBuilder;
    use crate::sub_agent::SubAgentCallbacks;
    use crate::super_agent::config::AgentID;
    use mockall::mock;
    use opamp_client::StartedClient;

    mock! {

        pub SubAgentEventProcessorBuilderMock<C>
        where
            C: StartedClient<SubAgentCallbacks> + 'static
        {}

        impl<C> SubAgentEventProcessorBuilder<C> for SubAgentEventProcessorBuilderMock<C>
         where
            C: StartedClient<SubAgentCallbacks> + 'static
        {
            type SubAgentEventProcessor = MockEventProcessorMock;

            fn build(
                &self,
                agent_id: AgentID,
                sub_agent_publisher: EventPublisher<SubAgentEvent>,
                sub_agent_opamp_consumer: EventConsumer<OpAMPEvent>,
                sub_agent_internal_consumer: EventConsumer<SubAgentInternalEvent>,
                maybe_opamp_client: Option<C>,
            ) -><Self as SubAgentEventProcessorBuilder<C>>::SubAgentEventProcessor;

        }
    }

    impl<C> MockSubAgentEventProcessorBuilderMock<C>
    where
        C: StartedClient<SubAgentCallbacks> + Send + Sync + 'static,
    {
        pub fn should_build(&mut self, processor: MockEventProcessorMock) {
            self.expect_build()
                .once()
                .return_once(move |_, _, _, _, _| processor);
        }
    }
}
