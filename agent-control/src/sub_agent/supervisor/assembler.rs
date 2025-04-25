use crate::agent_control::defaults::default_capabilities;
use crate::agent_control::run::Environment;
use crate::opamp::hash_repository::HashRepository;
use crate::opamp::remote_config::report::OpampRemoteConfigStatus;
use crate::sub_agent::effective_agents_assembler::{EffectiveAgent, EffectiveAgentsAssembler};
use crate::sub_agent::identity::AgentIdentity;
use crate::sub_agent::supervisor::builder::SupervisorBuilder;
use crate::sub_agent::supervisor::starter::SupervisorStarter;
use crate::values::yaml_config_repository::{
    YAMLConfigRepository, YAMLConfigRepositoryError, load_remote_fallback_local,
};
use opamp_client::StartedClient;
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, error, warn};

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
    hash_repository: Arc<HR>,
    supervisor_builder: B,
}

impl<HR, B> AgentSupervisorAssembler<HR, B>
where
    HR: HashRepository + Send + Sync + 'static,
    B: SupervisorBuilder,
{
    pub fn new(hash_repository: Arc<HR>, supervisor_builder: B) -> Self {
        Self {
            hash_repository,
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
        maybe_opamp_client: &Option<C>,
        agent_identity: AgentIdentity,
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
    use rstest::*;

    use crate::agent_control::agent_id::AgentID;
    use crate::agent_control::defaults::default_capabilities;
    use crate::agent_control::run::Environment;
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::agent_type::runtime_config::onhost::OnHost;
    use crate::agent_type::runtime_config::{Deployment, Runtime};
    use crate::opamp::client_builder::tests::MockStartedOpAMPClient;
    use crate::opamp::hash_repository::repository::HashRepositoryError;
    use crate::opamp::hash_repository::repository::tests::MockHashRepository;
    use crate::opamp::remote_config::hash::Hash;
    use crate::sub_agent::effective_agents_assembler::tests::MockEffectiveAgentAssembler;
    use crate::sub_agent::effective_agents_assembler::{
        self, EffectiveAgent, EffectiveAgentsAssemblerError,
    };
    use crate::sub_agent::identity::AgentIdentity;
    use crate::sub_agent::supervisor::assembler::{
        AgentSupervisorAssembler, SupervisorAssembler, SupervisorAssemblerError,
    };
    use crate::sub_agent::supervisor::builder::tests::MockSupervisorBuilder;
    use crate::sub_agent::supervisor::starter::SupervisorStarter;
    use crate::sub_agent::supervisor::starter::tests::MockSupervisorStarter;
    use crate::sub_agent::supervisor::stopper::tests::MockSupervisorStopper;
    use crate::values::yaml_config::YAMLConfig;
    use crate::values::yaml_config_repository::tests::MockYAMLConfigRepository;
    use mockall::mock;
    use opamp_client::StartedClient;
    use opamp_client::opamp::proto::RemoteConfigStatus;
    use opamp_client::opamp::proto::RemoteConfigStatuses::{Applied, Failed};
    use predicates::prelude::predicate;
    use std::sync::Arc;

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

    pub(crate) fn setup_hash_repository(
        hash: String,
        agent_identity: AgentIdentity,
    ) -> MockHashRepository {
        let mut hash_repository = MockHashRepository::new();
        if hash.is_empty() {
            hash_repository.should_not_get_hash(&agent_identity.id);
        } else {
            hash_repository.should_get_hash(&agent_identity.id, Hash::new(hash));
        }

        hash_repository
    }

    pub(crate) fn setup_effective_agent_assembler_to_return_ok(
        effective_agent: EffectiveAgent,
    ) -> MockEffectiveAgentAssembler {
        let mut effective_agent_assembler = MockEffectiveAgentAssembler::new();
        effective_agent_assembler
            .expect_assemble_agent()
            .once()
            .return_once(move |_, _, _| Ok(effective_agent));

        effective_agent_assembler
    }

    pub(crate) fn setup_effective_agent_assembler_to_return_err() -> MockEffectiveAgentAssembler {
        let mut effective_agent_assembler = MockEffectiveAgentAssembler::new();
        effective_agent_assembler
            .expect_assemble_agent()
            .once()
            .return_once(move |_, _, _| {
                Err(
                    EffectiveAgentsAssemblerError::EffectiveAgentsAssemblerError(String::from(
                        "random error!",
                    )),
                )
            });

        effective_agent_assembler
    }
}
