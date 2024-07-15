use crate::event::channel::{EventConsumer, EventPublisher};
use crate::event::{OpAMPEvent, SubAgentEvent, SubAgentInternalEvent};
use crate::opamp::effective_config::loader::EffectiveConfigLoader;
use crate::opamp::hash_repository::HashRepository;
use crate::sub_agent::event_processor::{EventProcessor, SubAgentEventProcessor};
use crate::sub_agent::SubAgentCallbacks;
use crate::super_agent::config::AgentID;
use crate::values::yaml_config_repository::YAMLConfigRepository;
use opamp_client::StartedClient;
use std::sync::Arc;

pub trait SubAgentEventProcessorBuilder<C, G>
where
    G: EffectiveConfigLoader,
    C: StartedClient<SubAgentCallbacks<G>> + 'static,
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
    R: YAMLConfigRepository,
{
    hash_repository: Arc<H>,
    yaml_config_repository: Arc<R>,
}

impl<H, R> EventProcessorBuilder<H, R>
where
    H: HashRepository,
    R: YAMLConfigRepository,
{
    pub fn new(
        hash_repository: Arc<H>,
        yaml_config_repository: Arc<R>,
    ) -> EventProcessorBuilder<H, R> {
        EventProcessorBuilder {
            yaml_config_repository,
            hash_repository,
        }
    }
}

impl<C, H, R, G> SubAgentEventProcessorBuilder<C, G> for EventProcessorBuilder<H, R>
where
    G: EffectiveConfigLoader,
    C: StartedClient<SubAgentCallbacks<G>> + 'static,
    H: HashRepository + Send + Sync + 'static,
    R: YAMLConfigRepository + Send + Sync + 'static,
{
    type SubAgentEventProcessor = EventProcessor<C, H, R, G>;

    fn build(
        &self,
        agent_id: AgentID,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
        sub_agent_opamp_consumer: Option<EventConsumer<OpAMPEvent>>,
        sub_agent_internal_consumer: EventConsumer<SubAgentInternalEvent>,
        maybe_opamp_client: Option<C>,
    ) -> EventProcessor<C, H, R, G>
    where
        G: EffectiveConfigLoader,
        C: StartedClient<SubAgentCallbacks<G>> + 'static,
    {
        EventProcessor::new(
            agent_id,
            sub_agent_publisher,
            sub_agent_opamp_consumer,
            sub_agent_internal_consumer,
            maybe_opamp_client,
            self.hash_repository.clone(),
            self.yaml_config_repository.clone(),
        )
    }
}

#[cfg(test)]
pub mod test {
    use crate::event::channel::{EventConsumer, EventPublisher};
    use crate::event::{OpAMPEvent, SubAgentEvent, SubAgentInternalEvent};
    use crate::opamp::effective_config::loader::tests::MockEffectiveConfigLoaderMock;
    use crate::sub_agent::event_processor::test::MockEventProcessorMock;
    use crate::sub_agent::event_processor_builder::SubAgentEventProcessorBuilder;
    use crate::sub_agent::SubAgentCallbacks;
    use crate::super_agent::config::AgentID;
    use mockall::mock;
    use opamp_client::StartedClient;
    use std::thread;

    mock! {

        pub SubAgentEventProcessorBuilderMock<C>
        where
            C: StartedClient<SubAgentCallbacks<MockEffectiveConfigLoaderMock>> + 'static
        {}

        impl<C> SubAgentEventProcessorBuilder<C, MockEffectiveConfigLoaderMock> for SubAgentEventProcessorBuilderMock<C>
         where
            C: StartedClient<SubAgentCallbacks<MockEffectiveConfigLoaderMock>> + 'static
        {
            type SubAgentEventProcessor = MockEventProcessorMock;

            fn build(
                &self,
                agent_id: AgentID,
                sub_agent_publisher: EventPublisher<SubAgentEvent>,
                sub_agent_opamp_consumer: Option<EventConsumer<OpAMPEvent>>,
                sub_agent_internal_consumer: EventConsumer<SubAgentInternalEvent>,
                maybe_opamp_client: Option<C>,
            ) -><Self as SubAgentEventProcessorBuilder<C, MockEffectiveConfigLoaderMock>>::SubAgentEventProcessor;

        }
    }

    impl<C> MockSubAgentEventProcessorBuilderMock<C>
    where
        C: StartedClient<SubAgentCallbacks<MockEffectiveConfigLoaderMock>> + Send + Sync + 'static,
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
