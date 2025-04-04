use std::sync::Arc;

use crate::agent_control::defaults::default_capabilities;
use crate::agent_control::run::Environment;
use crate::opamp::hash_repository::HashRepository;
use crate::opamp::remote_config::report::OpampRemoteConfigStatus;
use crate::sub_agent::effective_agents_assembler::EffectiveAgentsAssembler;
use crate::sub_agent::identity::AgentIdentity;
use crate::sub_agent::supervisor::builder::SupervisorBuilder;
use crate::sub_agent::supervisor::starter::SupervisorStarter;
use crate::values::yaml_config_repository::{
    load_remote_fallback_local, YAMLConfigRepository, YAMLConfigRepositoryError,
};
use opamp_client::StartedClient;
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
    ) -> Result<Self::SupervisorStarter, SupervisorAssemblerError>
    where
        C: StartedClient + Send + Sync + 'static;
}

/// SupervisorAssembler is an orchestrator to generate a Supervisor
/// It will use the EffectiveAgentAssembler and the HashRepository
/// to ensure that the Supervisor for the Sub Agent can be built.
/// If it succeeds, it will use the environment specific SupervisorBuilder
/// to actually build and return the Supervisor.
pub struct AgentSupervisorAssembler<HR, B, A, Y> {
    hash_repository: Arc<HR>,
    supervisor_builder: B,
    effective_agent_assembler: Arc<A>,
    yaml_config_repository: Arc<Y>,
    environment: Environment,
}

impl<HR, B, A, Y> AgentSupervisorAssembler<HR, B, A, Y>
where
    HR: HashRepository + Send + Sync + 'static,
    B: SupervisorBuilder,
    A: EffectiveAgentsAssembler,
    Y: YAMLConfigRepository,
{
    pub fn new(
        hash_repository: Arc<HR>,
        supervisor_builder: B,
        effective_agent_assembler: Arc<A>,
        yaml_config_repository: Arc<Y>,
        environment: Environment,
    ) -> Self {
        Self {
            hash_repository,
            supervisor_builder,
            effective_agent_assembler,
            yaml_config_repository,
            environment,
        }
    }
}

impl<HR, B, A, Y> SupervisorAssembler for AgentSupervisorAssembler<HR, B, A, Y>
where
    HR: HashRepository + Send + Sync + 'static,
    B: SupervisorBuilder,
    A: EffectiveAgentsAssembler,
    Y: YAMLConfigRepository,
{
    type SupervisorStarter = B::SupervisorStarter;

    fn assemble_supervisor<C>(
        &self,
        maybe_opamp_client: &Option<C>,
        agent_identity: AgentIdentity,
    ) -> Result<B::SupervisorStarter, SupervisorAssemblerError>
    where
        C: StartedClient + Send + Sync + 'static,
    {
        // Attempt to retrieve the hash
        let hash = self
            .hash_repository
            .get(&agent_identity.id)
            .inspect_err(|e| debug!( err = %e, "failed to get hash from repository"))
            .unwrap_or_default();

        if hash.is_none() {
            debug!("no previous remote config found");
        }

        // Load the configuration
        let Some(yaml_config) = load_remote_fallback_local(
            self.yaml_config_repository.as_ref(),
            &agent_identity.id,
            &default_capabilities(),
        )?
        else {
            debug!("there is no configuration for this agent");
            // TODO: instead of returning an error here, this method should probably receive an EffectiveAgent and
            // _assemble_ the corresponding supervisor only if the effective-agent was successfully put together.
            return Err(SupervisorAssemblerError::NoConfiguration);
        };

        // Assemble the new agent
        let effective_agent_result = self.effective_agent_assembler.assemble_agent(
            &agent_identity,
            yaml_config,
            &self.environment,
        );

        match effective_agent_result {
            Err(e) => {
                if let (Some(mut hash), Some(opamp_client)) = (hash, maybe_opamp_client) {
                    if !hash.is_failed() {
                        hash.fail(e.to_string());
                        _ = self
                            .hash_repository
                            .save(&agent_identity.id, &hash)
                            .inspect_err(|e| error!(err = %e, "failed to save hash to repository"));
                    }
                    _ = OpampRemoteConfigStatus::Error(e.to_string())
                        .report(opamp_client, &hash)
                        .inspect_err(|e| error!( %e, "error reporting remote config status"));
                }
                Err(SupervisorAssemblerError::AgentsAssemble(e.to_string()))
            }
            Ok(effective_agent) => {
                if let (Some(mut hash), Some(opamp_client)) = (hash, maybe_opamp_client) {
                    if hash.is_applying() {
                        debug!("applying remote config");
                        hash.apply();
                        _ = self
                            .hash_repository
                            .save(&agent_identity.id, &hash)
                            .inspect_err(
                                |e| error!( err = %e, "failed to save hash to repository"),
                            );
                        _ = opamp_client
                            .update_effective_config()
                            .inspect_err(|e| error!( %e, "effective config update failed"));
                        _ = OpampRemoteConfigStatus::Applied
                            .report(opamp_client, &hash)
                            .inspect_err(|e| error!( %e, "error reporting remote config status"));
                    }
                    if let Some(err) = hash.error_message() {
                        warn!( err = %err, "remote config failed. Building with previous stored config");
                        _ = OpampRemoteConfigStatus::Error(err)
                            .report(opamp_client, &hash)
                            .inspect_err(|e| error!( %e, "error reporting remote config status"));
                    }
                }
                let supervisor = self
                    .supervisor_builder
                    .build_supervisor(effective_agent)
                    .map_err(|e| SupervisorAssemblerError::SupervisorBuild(e.to_string()))?;

                Ok(supervisor)
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use crate::agent_control::agent_id::AgentID;
    use crate::agent_control::defaults::default_capabilities;
    use crate::agent_control::run::Environment;
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::agent_type::runtime_config::{Deployment, OnHost, Runtime};
    use crate::opamp::client_builder::tests::MockStartedOpAMPClientMock;
    use crate::opamp::hash_repository::repository::tests::MockHashRepositoryMock;
    use crate::opamp::hash_repository::repository::HashRepositoryError;
    use crate::opamp::remote_config::hash::Hash;
    use crate::sub_agent::effective_agents_assembler::tests::MockEffectiveAgentAssemblerMock;
    use crate::sub_agent::effective_agents_assembler::{
        EffectiveAgent, EffectiveAgentsAssemblerError,
    };
    use crate::sub_agent::identity::AgentIdentity;
    use crate::sub_agent::supervisor::assembler::{
        AgentSupervisorAssembler, SupervisorAssembler, SupervisorAssemblerError,
    };
    use crate::sub_agent::supervisor::builder::tests::MockSupervisorBuilder;
    use crate::sub_agent::supervisor::starter::tests::MockSupervisorStarter;
    use crate::sub_agent::supervisor::starter::SupervisorStarter;
    use crate::sub_agent::supervisor::stopper::tests::MockSupervisorStopper;
    use crate::values::yaml_config::YAMLConfig;
    use crate::values::yaml_config_repository::tests::MockYAMLConfigRepositoryMock;
    use assert_matches::assert_matches;
    use mockall::mock;
    use opamp_client::opamp::proto::RemoteConfigStatus;
    use opamp_client::opamp::proto::RemoteConfigStatuses::{Applied, Failed};
    use opamp_client::StartedClient;
    use predicates::prelude::predicate;
    use std::sync::Arc;

    //Mock implementation for tests
    mock! {
        pub SupervisorAssemblerMock<A> where A: SupervisorStarter + 'static {}

        impl<A> SupervisorAssembler for SupervisorAssemblerMock<A> where A:SupervisorStarter+ 'static{
            type SupervisorStarter = A;

            fn assemble_supervisor<C>(
                &self,
                maybe_opamp_client: &Option<C>,
                agent_identity: AgentIdentity,
            ) -> Result<A, SupervisorAssemblerError>
            where
                C: StartedClient + Send + Sync + 'static;
        }
    }

    impl MockSupervisorAssemblerMock<MockSupervisorStarter> {
        pub fn should_assemble<C>(
            &mut self,
            starter: MockSupervisorStarter,
            agent_identity: AgentIdentity,
        ) where
            C: StartedClient + Send + Sync + 'static,
        {
            self.expect_assemble_supervisor::<C>()
                .with(predicate::always(), predicate::eq(agent_identity))
                .once()
                .return_once(|_, _| Ok(starter));
        }
    }

    //Follow the same approach as before the refactor
    type AssemblerForTesting = AgentSupervisorAssembler<
        MockHashRepositoryMock,
        MockSupervisorBuilder<MockSupervisorStarter>,
        MockEffectiveAgentAssemblerMock,
        MockYAMLConfigRepositoryMock,
    >;

    type OpampClientForTest = MockStartedOpAMPClientMock;

    impl AssemblerForTesting {
        fn test_assembler(agent_identity: AgentIdentity) -> Self {
            let mut hash_repository = MockHashRepositoryMock::default();
            hash_repository
                .expect_get()
                .with(predicate::eq(agent_identity.id.clone()))
                .return_const(Ok(None));

            let mut yaml_config_repository = MockYAMLConfigRepositoryMock::new();
            yaml_config_repository.should_load_remote(
                &agent_identity.id,
                default_capabilities(),
                &YAMLConfig::default(),
            );

            let effective_agent = final_agent(agent_identity.clone());
            let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
            effective_agent_assembler.should_assemble_agent(
                &agent_identity,
                &YAMLConfig::default(),
                &Environment::OnHost,
                effective_agent.clone(),
                1,
            );

            let mut supervisor_stopper = MockSupervisorStopper::new();
            supervisor_stopper
                .expect_stop()
                .times(0..=1) // at most once
                .return_once(|| Ok(()));

            let mut supervisor_starter = MockSupervisorStarter::new();
            supervisor_starter
                .expect_start()
                .times(0..=1) // at most once
                .with(predicate::always())
                .return_once(|_| Ok(supervisor_stopper));

            let mut supervisor_builder = MockSupervisorBuilder::new();
            supervisor_builder
                .expect_build_supervisor()
                .with(predicate::function(move |e: &EffectiveAgent| {
                    e == &effective_agent
                }))
                .return_once(|_| Ok(supervisor_starter));

            let hash_repository_ref = Arc::new(hash_repository);

            AgentSupervisorAssembler::new(
                hash_repository_ref,
                supervisor_builder,
                Arc::new(effective_agent_assembler),
                Arc::new(yaml_config_repository),
                Environment::OnHost,
            )
        }
    }

    // Tests for `assemble_supervisor` function
    // Essentially, the function defines the behavior for a certain combination
    // of the following parameters:
    //
    // - The presence of an OpAMP client. Can be either `Some(opamp_client)` or `None`.
    // - The presence of a hash in the hash repository for the given agent_id: The call to `hash_repository.get(agent_id)?` done inside the function returns either `Some(Hash)` or `None`.
    // - The result of the agent assembly attempt. Can be either `Ok(EffectiveAgent)` or `Err(EffectiveAgentsAssemblerError)`.
    //
    // When the OpAMP client is `None` the function `hash_repository.get(agent_id)?` won't be called, there's no value to check for.
    // We are safe to discard those from the testing set and only look at the effective agent assemble result in this case.
    //
    // So, we cover all cases.

    /// `maybe_opamp_client == Some(_)`
    /// `hash_repository.get(agent_id)? == Some(_)`
    /// `effective_agent_res == Ok(_)`
    #[test]
    fn test_assemble_supervisor_from_some_hash_ok_eff_agent() {
        let agent_identity = AgentIdentity::from((
            AgentID::new("some-agent-id").unwrap(),
            AgentTypeID::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        ));
        //  create a default assembler
        let mut assembler = AssemblerForTesting::test_assembler(agent_identity.clone());

        // Modify expectations for this test
        // Expected calls on the hash repository
        let hash = Hash::new("some_hash".to_string());
        let mut applied_hash = hash.clone();
        applied_hash.apply();
        let mut hash_repository = MockHashRepositoryMock::new();
        hash_repository.should_get_hash(&agent_identity.id, hash);
        hash_repository.should_save_hash(&agent_identity.id, &applied_hash);

        assembler.hash_repository = Arc::new(hash_repository);

        // Expected calls on the opamp client
        let mut started_opamp_client = OpampClientForTest::new();

        started_opamp_client.should_set_remote_config_status(RemoteConfigStatus {
            last_remote_config_hash: "some_hash".as_bytes().to_vec(),
            status: Applied as i32,
            error_message: "".to_string(),
        });

        started_opamp_client.should_update_effective_config(1);
        let maybe_opamp_client = Some(started_opamp_client);

        assert!(assembler
            .assemble_supervisor(&maybe_opamp_client, agent_identity)
            .is_ok());
    }

    /// `maybe_opamp_client == Some(_)`
    /// `hash_repository.get(agent_id) fails` must not be different from the `None` cases, but we test it anyway to detect if this invariant changes
    /// `effective_agent_res == Ok(_)`
    #[test]
    fn test_assemble_supervisor_from_err_hash_ok_eff_agent() {
        let agent_identity = AgentIdentity::from((
            AgentID::new("some-agent-id").unwrap(),
            AgentTypeID::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        ));
        //  create a default assembler
        let mut assembler = AssemblerForTesting::test_assembler(agent_identity.clone());

        // Modify expectations for this test
        // Expected calls on the hash repository
        let mut hash_repository = MockHashRepositoryMock::new();
        hash_repository.should_return_error_on_get(
            &agent_identity.id,
            HashRepositoryError::LoadError(String::from("random error loading")),
        );

        assembler.hash_repository = Arc::new(hash_repository);

        // Expected calls on the opamp client
        let maybe_opamp_client = Some(OpampClientForTest::new());

        assert!(assembler
            .assemble_supervisor(&maybe_opamp_client, agent_identity)
            .is_ok());
    }

    /// `maybe_opamp_client == Some(_)`
    /// `hash_repository.get(agent_id)? == Some(_)`
    /// `effective_agent_res == Err(_)`
    #[test]
    fn test_assemble_supervisor_from_some_hash_err_eff_agent() {
        let agent_identity = AgentIdentity::from((
            AgentID::new("some-agent-id").unwrap(),
            AgentTypeID::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        ));

        let mut hash = Hash::new("some_hash".to_string());
        hash.fail("error assembling agents: `a random error happened!`".to_string());

        let expected_remote_config_status = RemoteConfigStatus {
            last_remote_config_hash: hash.get().as_bytes().to_vec(),
            status: Failed as i32,
            error_message: hash.error_message().unwrap(),
        };

        let mut hash_repository = MockHashRepositoryMock::new();
        hash_repository.should_get_hash(&agent_identity.id, hash);

        let mut yaml_config_repository = MockYAMLConfigRepositoryMock::new();
        yaml_config_repository.should_load_remote(
            &agent_identity.id,
            default_capabilities(),
            &YAMLConfig::default(),
        );

        let effective_agent = final_agent(agent_identity.clone());

        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
        effective_agent_assembler
            .expect_assemble_agent()
            .once()
            .returning(|_, _, _| {
                Err(
                    EffectiveAgentsAssemblerError::EffectiveAgentsAssemblerError(String::from(
                        "a random error happened!",
                    )),
                )
            });

        let mut supervisor_builder = MockSupervisorBuilder::new();
        supervisor_builder
            .expect_build_supervisor()
            .with(predicate::function(move |e: &EffectiveAgent| {
                e == &effective_agent
            }))
            .return_once(|_| Ok(MockSupervisorStarter::new()));

        let mut opamp_client = OpampClientForTest::new();
        opamp_client.should_set_remote_config_status(expected_remote_config_status);

        let hash_repository_ref = Arc::new(hash_repository);

        let supervisor_assembler = AgentSupervisorAssembler::new(
            hash_repository_ref,
            supervisor_builder,
            Arc::new(effective_agent_assembler),
            Arc::new(yaml_config_repository),
            Environment::OnHost,
        );

        let maybe_opamp_client = Some(opamp_client);

        assert!(supervisor_assembler
            .assemble_supervisor(&maybe_opamp_client, agent_identity)
            .is_err());
    }

    /// `maybe_opamp_client == Some(_)`
    /// `hash_repository.get(agent_id)? == None`
    /// `effective_agent_res == Ok(_)`
    #[test]
    fn test_assemble_supervisor_from_none_hash_ok_eff_agent() {
        let agent_identity = AgentIdentity::from((
            AgentID::new("some-agent-id").unwrap(),
            AgentTypeID::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        ));

        let mut hash_repository = MockHashRepositoryMock::new();
        hash_repository.should_not_get_hash(&agent_identity.id);

        let mut yaml_config_repository = MockYAMLConfigRepositoryMock::new();
        yaml_config_repository.should_load_remote(
            &agent_identity.id,
            default_capabilities(),
            &YAMLConfig::default(),
        );

        let effective_agent = final_agent(agent_identity.clone());
        let assembled_effective_agent = effective_agent.clone();

        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
        effective_agent_assembler
            .expect_assemble_agent()
            .once()
            .return_once(move |_, _, _| Ok(assembled_effective_agent));

        let mut supervisor_builder = MockSupervisorBuilder::new();
        supervisor_builder
            .expect_build_supervisor()
            .with(predicate::function(move |e: &EffectiveAgent| {
                e == &effective_agent
            }))
            .return_once(|_| Ok(MockSupervisorStarter::new()));

        let opamp_client = OpampClientForTest::new();

        let hash_repository_ref = Arc::new(hash_repository);

        let supervisor_assembler = AgentSupervisorAssembler::new(
            hash_repository_ref,
            supervisor_builder,
            Arc::new(effective_agent_assembler),
            Arc::new(yaml_config_repository),
            Environment::OnHost,
        );

        let maybe_opamp_client = Some(opamp_client);

        assert!(supervisor_assembler
            .assemble_supervisor(&maybe_opamp_client, agent_identity)
            .is_ok());
    }

    /// `maybe_opamp_client == Some(_)`
    /// `hash_repository.get(agent_id)? == None`
    /// `effective_agent_res == Err(_)`
    #[test]
    fn test_assemble_supervisor_from_none_hash_err_eff_agent() {
        let agent_identity = AgentIdentity::from((
            AgentID::new("some-agent-id").unwrap(),
            AgentTypeID::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        ));

        let mut hash_repository = MockHashRepositoryMock::new();
        hash_repository.should_not_get_hash(&agent_identity.id);

        let mut yaml_config_repository = MockYAMLConfigRepositoryMock::new();
        yaml_config_repository.should_load_remote(
            &agent_identity.id,
            default_capabilities(),
            &YAMLConfig::default(),
        );

        let effective_agent = final_agent(agent_identity.clone());

        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
        effective_agent_assembler
            .expect_assemble_agent()
            .once()
            .returning(|_, _, _| {
                Err(
                    EffectiveAgentsAssemblerError::EffectiveAgentsAssemblerError(String::from(
                        "a random error happened!",
                    )),
                )
            });

        let mut supervisor_builder = MockSupervisorBuilder::new();
        supervisor_builder
            .expect_build_supervisor()
            .with(predicate::function(move |e: &EffectiveAgent| {
                e == &effective_agent
            }))
            .return_once(|_| Ok(MockSupervisorStarter::new()));

        let opamp_client = OpampClientForTest::new();

        let hash_repository_ref = Arc::new(hash_repository);

        let supervisor_assembler = AgentSupervisorAssembler::new(
            hash_repository_ref,
            supervisor_builder,
            Arc::new(effective_agent_assembler),
            Arc::new(yaml_config_repository),
            Environment::OnHost,
        );

        let maybe_opamp_client = Some(opamp_client);

        assert!(supervisor_assembler
            .assemble_supervisor(&maybe_opamp_client, agent_identity)
            .is_err());
    }

    /// `maybe_opamp_client == None`
    /// `hash_repository.get(agent_id)? == Some(_)
    /// `effective_agent_res == Ok(_)`
    #[test]
    fn test_assemble_supervisor_from_ok_eff_agent_no_opamp() {
        let agent_identity = AgentIdentity::from((
            AgentID::new("some-agent-id").unwrap(),
            AgentTypeID::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        ));

        let hash = Hash::new("some_hash".to_string());
        let mut hash_repository = MockHashRepositoryMock::new();
        hash_repository.should_get_hash(&agent_identity.id, hash);

        let mut yaml_config_repository = MockYAMLConfigRepositoryMock::new();
        yaml_config_repository.should_load_remote(
            &agent_identity.id,
            default_capabilities(),
            &YAMLConfig::default(),
        );

        let effective_agent = final_agent(agent_identity.clone());
        let assembled_effective_agent = effective_agent.clone();

        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
        effective_agent_assembler
            .expect_assemble_agent()
            .once()
            .return_once(move |_, _, _| Ok(assembled_effective_agent));

        let mut supervisor_builder = MockSupervisorBuilder::new();
        supervisor_builder
            .expect_build_supervisor()
            .with(predicate::function(move |e: &EffectiveAgent| {
                e == &effective_agent
            }))
            .return_once(|_| Ok(MockSupervisorStarter::new()));

        let hash_repository_ref = Arc::new(hash_repository);

        let supervisor_assembler = AgentSupervisorAssembler::new(
            hash_repository_ref,
            supervisor_builder,
            Arc::new(effective_agent_assembler),
            Arc::new(yaml_config_repository),
            Environment::OnHost,
        );

        let maybe_opamp_client: Option<OpampClientForTest> = None;

        assert!(supervisor_assembler
            .assemble_supervisor(&maybe_opamp_client, agent_identity)
            .is_ok());
    }

    /// `maybe_opamp_client == None`
    /// `hash_repository.get(agent_id)? == None
    /// `effective_agent_res == Ok(_)`
    #[test]
    fn test_assemble_supervisor_from_ok_eff_agent_no_opamp_no_hash() {
        let agent_identity = AgentIdentity::from((
            AgentID::new("some-agent-id").unwrap(),
            AgentTypeID::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        ));

        let mut hash_repository = MockHashRepositoryMock::new();
        hash_repository.should_not_get_hash(&agent_identity.id);

        let mut yaml_config_repository = MockYAMLConfigRepositoryMock::new();
        yaml_config_repository.should_load_remote(
            &agent_identity.id,
            default_capabilities(),
            &YAMLConfig::default(),
        );

        let effective_agent = final_agent(agent_identity.clone());
        let assembled_effective_agent = effective_agent.clone();

        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
        effective_agent_assembler
            .expect_assemble_agent()
            .once()
            .return_once(move |_, _, _| Ok(assembled_effective_agent));

        let mut supervisor_builder = MockSupervisorBuilder::new();
        supervisor_builder
            .expect_build_supervisor()
            .with(predicate::function(move |e: &EffectiveAgent| {
                e == &effective_agent
            }))
            .return_once(|_| Ok(MockSupervisorStarter::new()));

        let hash_repository_ref = Arc::new(hash_repository);

        let supervisor_assembler = AgentSupervisorAssembler::new(
            hash_repository_ref,
            supervisor_builder,
            Arc::new(effective_agent_assembler),
            Arc::new(yaml_config_repository),
            Environment::OnHost,
        );

        let maybe_opamp_client: Option<OpampClientForTest> = None;

        assert!(supervisor_assembler
            .assemble_supervisor(&maybe_opamp_client, agent_identity)
            .is_ok());
    }

    /// `maybe_opamp_client == None`
    /// `hash_repository.get(agent_id)? == Some(_)
    /// `effective_agent_res == Err(_)`
    #[test]
    fn test_assemble_supervisor_from_err_eff_agent_no_opamp() {
        let agent_identity = AgentIdentity::from((
            AgentID::new("some-agent-id").unwrap(),
            AgentTypeID::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        ));

        let hash = Hash::new("some_hash".to_string());
        let mut hash_repository = MockHashRepositoryMock::new();
        hash_repository.should_get_hash(&agent_identity.id, hash);

        let mut yaml_config_repository = MockYAMLConfigRepositoryMock::new();
        yaml_config_repository.should_load_remote(
            &agent_identity.id,
            default_capabilities(),
            &YAMLConfig::default(),
        );

        let effective_agent = final_agent(agent_identity.clone());

        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
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

        let mut supervisor_builder = MockSupervisorBuilder::new();
        supervisor_builder
            .expect_build_supervisor()
            .with(predicate::function(move |e: &EffectiveAgent| {
                e == &effective_agent
            }))
            .return_once(|_| Ok(MockSupervisorStarter::new()));

        let hash_repository_ref = Arc::new(hash_repository);

        let supervisor_assembler = AgentSupervisorAssembler::new(
            hash_repository_ref,
            supervisor_builder,
            Arc::new(effective_agent_assembler),
            Arc::new(yaml_config_repository),
            Environment::OnHost,
        );

        let maybe_opamp_client: Option<OpampClientForTest> = None;

        assert!(supervisor_assembler
            .assemble_supervisor(&maybe_opamp_client, agent_identity)
            .is_err());
    }

    /// `maybe_opamp_client == None`
    /// `hash_repository.get(agent_id)? == None
    /// `effective_agent_res == Err(_)`
    #[test]
    fn test_assemble_supervisor_from_err_eff_agent_no_opamp_no_hash() {
        let agent_identity = AgentIdentity::from((
            AgentID::new("some-agent-id").unwrap(),
            AgentTypeID::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        ));

        let mut hash_repository = MockHashRepositoryMock::new();
        hash_repository.should_not_get_hash(&agent_identity.id);

        let mut yaml_config_repository = MockYAMLConfigRepositoryMock::new();
        yaml_config_repository.should_load_remote(
            &agent_identity.id,
            default_capabilities(),
            &YAMLConfig::default(),
        );

        let effective_agent = final_agent(agent_identity.clone());

        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
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

        let mut supervisor_builder = MockSupervisorBuilder::new();
        supervisor_builder
            .expect_build_supervisor()
            .with(predicate::function(move |e: &EffectiveAgent| {
                e == &effective_agent
            }))
            .return_once(|_| Ok(MockSupervisorStarter::new()));

        let hash_repository_ref = Arc::new(hash_repository);

        let supervisor_assembler = AgentSupervisorAssembler::new(
            hash_repository_ref,
            supervisor_builder,
            Arc::new(effective_agent_assembler),
            Arc::new(yaml_config_repository),
            Environment::OnHost,
        );

        let maybe_opamp_client: Option<OpampClientForTest> = None;

        assert!(supervisor_assembler
            .assemble_supervisor(&maybe_opamp_client, agent_identity)
            .is_err());
    }

    #[test]
    fn test_assemble_supervisor_yaml_values_error() {
        let agent_identity = AgentIdentity::from((
            AgentID::new("some-agent-id").unwrap(),
            AgentTypeID::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        ));

        let mut hash_repository = MockHashRepositoryMock::new();
        hash_repository.should_not_get_hash(&agent_identity.id);

        let mut yaml_config_repository = MockYAMLConfigRepositoryMock::new();
        yaml_config_repository.should_not_load_remote(&agent_identity.id, default_capabilities());

        let effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
        let supervisor_builder: MockSupervisorBuilder<MockSupervisorStarter> =
            MockSupervisorBuilder::new();
        let hash_repository_ref = Arc::new(hash_repository);

        let supervisor_assembler = AgentSupervisorAssembler::new(
            hash_repository_ref,
            supervisor_builder,
            Arc::new(effective_agent_assembler),
            Arc::new(yaml_config_repository),
            Environment::OnHost,
        );

        let maybe_opamp_client: Option<OpampClientForTest> = None;

        assert!(supervisor_assembler
            .assemble_supervisor(&maybe_opamp_client, agent_identity)
            .is_err());
    }

    #[test]
    fn test_assemble_supervisor_yaml_values_none() {
        let agent_identity = AgentIdentity::from((
            AgentID::new("some-agent-id").unwrap(),
            AgentTypeID::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        ));

        let mut hash_repository = MockHashRepositoryMock::new();
        hash_repository.should_not_get_hash(&agent_identity.id);

        let mut yaml_config_repository = MockYAMLConfigRepositoryMock::new();
        // There is no local nor remote configuration
        yaml_config_repository
            .expect_load_remote()
            .once()
            .with(
                predicate::eq(agent_identity.id.clone()),
                predicate::eq(default_capabilities()),
            )
            .returning(move |_, _| Ok(None));
        yaml_config_repository
            .expect_load_local()
            .once()
            .with(predicate::eq(agent_identity.id.clone()))
            .returning(move |_| Ok(None));

        let effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
        let supervisor_builder: MockSupervisorBuilder<MockSupervisorStarter> =
            MockSupervisorBuilder::new();
        let hash_repository_ref = Arc::new(hash_repository);

        let supervisor_assembler = AgentSupervisorAssembler::new(
            hash_repository_ref,
            supervisor_builder,
            Arc::new(effective_agent_assembler),
            Arc::new(yaml_config_repository),
            Environment::OnHost,
        );

        let maybe_opamp_client: Option<OpampClientForTest> = None;

        let supervisor_result =
            supervisor_assembler.assemble_supervisor(&maybe_opamp_client, agent_identity);
        assert_matches!(
            supervisor_result.err().unwrap(),
            SupervisorAssemblerError::NoConfiguration
        );
    }

    fn final_agent(agent_identity: AgentIdentity) -> EffectiveAgent {
        EffectiveAgent::new(
            agent_identity,
            Runtime {
                deployment: Deployment {
                    on_host: Some(OnHost::default()),
                    k8s: None,
                },
            },
        )
    }
}
