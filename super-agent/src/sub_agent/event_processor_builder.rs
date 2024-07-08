use crate::event::channel::{EventConsumer, EventPublisher};
use crate::event::{OpAMPEvent, SubAgentEvent, SubAgentInternalEvent};
use crate::opamp::hash_repository::HashRepository;
use crate::sub_agent::event_processor::{EventProcessor, SubAgentEventProcessor};
use crate::sub_agent::values::values_repository::ValuesRepository;
use crate::sub_agent::SubAgentCallbacks;
use crate::super_agent::config::AgentID;
use opamp_client::StartedClient;
use std::sync::Arc;

pub trait SubAgentEventProcessorBuilder<C, R>
where
    R: ValuesRepository,
    C: StartedClient<SubAgentCallbacks<R>> + 'static,
{
    type SubAgentEventProcessor: SubAgentEventProcessor;

    fn build(
        &self,
        agent_id: AgentID,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
        sub_agent_opamp_consumer: Option<EventConsumer<OpAMPEvent>>,
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

impl<C, H, R> SubAgentEventProcessorBuilder<C, R> for EventProcessorBuilder<H, R>
where
    C: StartedClient<SubAgentCallbacks<R>> + 'static,
    H: HashRepository + Send + Sync + 'static,
    R: ValuesRepository + Send + Sync + 'static,
{
    type SubAgentEventProcessor = EventProcessor<C, H, R>;

    fn build(
        &self,
        agent_id: AgentID,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
        sub_agent_opamp_consumer: Option<EventConsumer<OpAMPEvent>>,
        sub_agent_internal_consumer: EventConsumer<SubAgentInternalEvent>,
        maybe_opamp_client: Option<C>,
    ) -> EventProcessor<C, H, R>
    where
        C: StartedClient<SubAgentCallbacks<R>> + 'static,
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
    use crate::sub_agent::values::values_repository::ValuesRepository;
    use crate::sub_agent::SubAgentCallbacks;
    use crate::super_agent::config::AgentID;
    use mockall::mock;
    use opamp_client::StartedClient;
    use std::thread;

    mock! {

        pub SubAgentEventProcessorBuilderMock<C,R>
        where
            R: ValuesRepository,
            C: StartedClient<SubAgentCallbacks<R>> + 'static
        {}

        impl<C,R> SubAgentEventProcessorBuilder<C,R> for SubAgentEventProcessorBuilderMock<C,R>
         where
            R: ValuesRepository,
            C: StartedClient<SubAgentCallbacks<R>> + 'static
        {
            type SubAgentEventProcessor = MockEventProcessorMock;

            fn build(
                &self,
                agent_id: AgentID,
                sub_agent_publisher: EventPublisher<SubAgentEvent>,
                sub_agent_opamp_consumer: Option<EventConsumer<OpAMPEvent>>,
                sub_agent_internal_consumer: EventConsumer<SubAgentInternalEvent>,
                maybe_opamp_client: Option<C>,
            ) -><Self as SubAgentEventProcessorBuilder<C,R>>::SubAgentEventProcessor;

        }
    }

    impl<C, R> MockSubAgentEventProcessorBuilderMock<C, R>
    where
        R: ValuesRepository,
        C: StartedClient<SubAgentCallbacks<R>> + Send + Sync + 'static,
    {
        pub fn should_build(&mut self, processor: MockEventProcessorMock) {
            self.expect_build()
                .once()
                .return_once(move |_, _, _, _, _| processor);
        }

        pub fn should_return_event_processor_with_consumer(&mut self) {
            let mut sub_agent_event_processor = MockEventProcessorMock::default();
            sub_agent_event_processor.should_process();

            self.expect_build()
                .once()
                .return_once(move |_, _, _, consumer, _| {
                    thread::spawn(move || {
                        _ = consumer.as_ref().recv();
                    });
                    sub_agent_event_processor
                });
        }
    }
}
