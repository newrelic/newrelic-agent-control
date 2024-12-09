use crate::sub_agent::effective_agents_assembler::EffectiveAgent;
use crate::sub_agent::error::SubAgentBuilderError;
use crate::sub_agent::supervisor::starter::SupervisorStarter;

pub trait SupervisorBuilder {
    type SupervisorStarter: SupervisorStarter;
    type OpAMPClient;

    fn build_supervisor(
        &self,
        effective_agent: EffectiveAgent,
    ) -> Result<Self::SupervisorStarter, SubAgentBuilderError>;
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::opamp::callbacks::AgentCallbacks;
    use crate::opamp::client_builder::tests::MockStartedOpAMPClientMock;
    use crate::opamp::effective_config::loader::tests::MockEffectiveConfigLoaderMock;
    use crate::sub_agent::effective_agents_assembler::EffectiveAgent;
    use crate::sub_agent::error::SubAgentBuilderError;
    use crate::sub_agent::supervisor::builder::SupervisorBuilder;
    use crate::sub_agent::supervisor::starter::SupervisorStarter;
    use mockall::mock;

    mock! {
        pub SupervisorBuilder<A> where A: SupervisorStarter {}

        impl<A> SupervisorBuilder for SupervisorBuilder<A> where A: SupervisorStarter {
            type SupervisorStarter = A;
            type OpAMPClient = MockStartedOpAMPClientMock<AgentCallbacks<MockEffectiveConfigLoaderMock>>;

            fn build_supervisor(
                &self,
                effective_agent: EffectiveAgent,
            ) -> Result<A, SubAgentBuilderError>;
        }
    }
}
