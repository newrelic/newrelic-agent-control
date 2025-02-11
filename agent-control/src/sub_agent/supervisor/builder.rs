use opamp_client::StartedClient;

use crate::sub_agent::effective_agents_assembler::EffectiveAgent;
use crate::sub_agent::error::SubAgentBuilderError;
use crate::sub_agent::supervisor::starter::SupervisorStarter;

pub trait SupervisorBuilder<C>
where
    C: StartedClient + Send + Sync + 'static,
{
    type SupervisorStarter: SupervisorStarter<C>;

    fn build_supervisor(
        &self,
        effective_agent: EffectiveAgent,
    ) -> Result<Self::SupervisorStarter, SubAgentBuilderError>;
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::sub_agent::effective_agents_assembler::EffectiveAgent;
    use crate::sub_agent::error::SubAgentBuilderError;
    use crate::sub_agent::supervisor::builder::SupervisorBuilder;
    use crate::sub_agent::supervisor::starter::SupervisorStarter;
    use mockall::mock;
    use opamp_client::StartedClient;

    mock! {
        pub SupervisorBuilder<A, C> where A: SupervisorStarter<C>, C: StartedClient + Send + Sync + 'static {}

        impl<A, C> SupervisorBuilder<C> for SupervisorBuilder<A, C> where A: SupervisorStarter<C>, C: StartedClient + Send + Sync + 'static {
            type SupervisorStarter = A;

            fn build_supervisor(
                &self,
                effective_agent: EffectiveAgent,
            ) -> Result<A, SubAgentBuilderError>;
        }
    }
}
