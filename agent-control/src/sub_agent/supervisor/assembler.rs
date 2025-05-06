use crate::opamp::hash_repository::HashRepository;
use crate::sub_agent::effective_agents_assembler::EffectiveAgent;
use crate::sub_agent::identity::AgentIdentity;
use crate::sub_agent::supervisor::builder::SupervisorBuilder;
use crate::sub_agent::supervisor::starter::SupervisorStarter;
use crate::values::yaml_config_repository::YAMLConfigRepositoryError;
use opamp_client::StartedClient;
use std::sync::Arc;
use thiserror::Error;
use tracing::error;

#[derive(Debug, Error)]
pub enum SupervisorAssemblerError {
    #[error("error assembling agent: `{0}`")]
    AgentsAssemble(String),

    #[error("supervisor could not be built: `{0}`")]
    SupervisorBuild(String),

    #[error("values error: {0}")]
    YAMLConfigRepository(#[from] YAMLConfigRepositoryError),

    #[error("no configuration found")]
    NoConfiguration,
}

pub trait SupervisorAssembler {
    type SupervisorStarter: SupervisorStarter;
    fn assemble_supervisor<C>(
        &self,
        maybe_opamp_client: &Option<C>,
        agent_identity: AgentIdentity,
        supervisor_config: EffectiveAgent,
    ) -> Result<Self::SupervisorStarter, SupervisorAssemblerError>
    where
        C: StartedClient + Send + Sync + 'static;
}

/// SupervisorAssembler is an orchestrator to generate a Supervisor
/// It will use the EffectiveAgentAssembler and the HashRepository
/// to ensure that the Supervisor for the Sub Agent can be built.
/// If it succeeds, it will use the environment specific SupervisorBuilder
/// to actually build and return the Supervisor.
pub struct AgentSupervisorAssembler<HR, B> {
    _hash_repository: Arc<HR>,
    supervisor_builder: B,
}

impl<HR, B> AgentSupervisorAssembler<HR, B>
where
    HR: HashRepository + Send + Sync + 'static,
    B: SupervisorBuilder,
{
    pub fn new(hash_repository: Arc<HR>, supervisor_builder: B) -> Self {
        Self {
            _hash_repository: hash_repository,
            supervisor_builder,
        }
    }
}

impl<HR, B> SupervisorAssembler for AgentSupervisorAssembler<HR, B>
where
    HR: HashRepository + Send + Sync + 'static,
    B: SupervisorBuilder,
{
    type SupervisorStarter = B::SupervisorStarter;

    fn assemble_supervisor<C>(
        &self,
        _maybe_opamp_client: &Option<C>,
        _agent_identity: AgentIdentity,
        effective_agent: EffectiveAgent,
    ) -> Result<B::SupervisorStarter, SupervisorAssemblerError>
    where
        C: StartedClient + Send + Sync + 'static,
    {
        let supervisor = self
            .supervisor_builder
            .build_supervisor(effective_agent)
            .map_err(|e| SupervisorAssemblerError::SupervisorBuild(e.to_string()))?;

        Ok(supervisor)
    }
}

#[cfg(test)]
pub mod tests {

    use crate::sub_agent::effective_agents_assembler::EffectiveAgent;
    use crate::sub_agent::identity::AgentIdentity;
    use crate::sub_agent::supervisor::assembler::{SupervisorAssembler, SupervisorAssemblerError};
    use crate::sub_agent::supervisor::starter::SupervisorStarter;
    use crate::sub_agent::supervisor::starter::tests::MockSupervisorStarter;
    use mockall::mock;
    use opamp_client::StartedClient;
    use predicates::prelude::predicate;

    //Mock implementation for tests
    mock! {
        pub SupervisorAssembler<A> where A: SupervisorStarter + 'static {}

        impl<A> SupervisorAssembler for SupervisorAssembler<A> where A:SupervisorStarter+ 'static{
            type SupervisorStarter = A;

            fn assemble_supervisor<C>(
                &self,
                maybe_opamp_client: &Option<C>,
                agent_identity: AgentIdentity,
                effective_agent: EffectiveAgent,
            ) -> Result<A, SupervisorAssemblerError>
            where
                C: StartedClient + Send + Sync + 'static;
        }
    }

    impl MockSupervisorAssembler<MockSupervisorStarter> {
        pub fn should_assemble<C>(
            &mut self,
            starter: MockSupervisorStarter,
            agent_identity: AgentIdentity,
            effective_agent: EffectiveAgent,
        ) where
            C: StartedClient + Send + Sync + 'static,
        {
            self.expect_assemble_supervisor::<C>()
                .with(
                    predicate::always(),
                    predicate::eq(agent_identity),
                    predicate::eq(effective_agent),
                )
                .once()
                .return_once(|_, _, _| Ok(starter));
        }
    }
}
