use crate::agent_control::config::{AgentID, SubAgentConfig};
use crate::agent_type::environment::Environment;
use crate::opamp::remote_config::report::OpampRemoteConfigStatus;
use crate::opamp::remote_config::status::AgentRemoteConfigStatus;
use crate::opamp::remote_config::status_manager::ConfigStatusManager;
use crate::sub_agent::effective_agents_assembler::EffectiveAgentsAssembler;
use crate::sub_agent::supervisor::builder::SupervisorBuilder;
use opamp_client::operation::callbacks::Callbacks;
use opamp_client::StartedClient;
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, error, warn};

#[derive(Debug, Error)]
pub enum SupervisorAssemblerError {
    #[error("error assembling agent: `{0}`")]
    AgentAssembleError(String),

    #[error("supervisor could not be built: `{0}`")]
    SupervisorBuildError(String),
}

/// SupervisorAssembler is an orchestrator to generate a Supervisor
/// It will use the EffectiveAgentAssembler and the HashRepository
/// to ensure that the Supervisor for the Sub Agent can be built.
/// If it succeeds, it will use the environment specific SupervisorBuilder
/// to actually build and return the Supervisor.
pub struct SupervisorAssembler<M, B, A> {
    config_status_manager: Arc<M>,
    supervisor_builder: B,
    agent_id: AgentID,
    agent_cfg: SubAgentConfig,
    effective_agent_assembler: Arc<A>,
    environment: Environment,
}

impl<M, B, A> SupervisorAssembler<M, B, A>
where
    M: ConfigStatusManager + Send + Sync + 'static,
    B: SupervisorBuilder,
    A: EffectiveAgentsAssembler,
{
    pub fn new(
        config_status_manager: Arc<M>,
        supervisor_builder: B,
        agent_id: AgentID,
        agent_cfg: SubAgentConfig,
        effective_agent_assembler: Arc<A>,
        environment: Environment,
    ) -> Self {
        Self {
            config_status_manager,
            supervisor_builder,
            agent_id,
            agent_cfg,
            effective_agent_assembler,
            environment,
        }
    }

    pub fn assemble_supervisor<CB, C>(
        &self,
        maybe_opamp_client: &Option<C>,
        maybe_remote_status: Option<AgentRemoteConfigStatus>,
    ) -> Result<B::SupervisorStarter, SupervisorAssemblerError>
    where
        CB: Callbacks + Send + Sync + 'static,
        C: StartedClient<CB> + Send + Sync + 'static,
    {
        let maybe_remote_config = maybe_remote_status
            .as_ref()
            .and_then(|s| s.remote_config.clone());

        if maybe_remote_config.is_none() {
            debug!(%self.agent_id, "no remote config found");
        }

        // Assemble the new agent
        let effective_agent_result = self.effective_agent_assembler.assemble_agent(
            &self.agent_id,
            &self.agent_cfg,
            &self.environment,
            maybe_remote_config,
        );

        match effective_agent_result {
            Err(e) => {
                if let (Some(mut remote_status), Some(opamp_client)) =
                    (maybe_remote_status, maybe_opamp_client)
                {
                    remote_status.status_hash.fail(e.to_string());
                    _ = self
                        .config_status_manager
                        .store_remote_status(&self.agent_id, &remote_status)
                        .inspect_err(
                            |e| error!(%self.agent_id, err = %e, "failed to store remote status"),
                        );
                    _ = OpampRemoteConfigStatus::Error(e.to_string())
                        .report(opamp_client, &remote_status.status_hash)
                        .inspect_err(
                            |e| error!(%self.agent_id, %e, "error reporting remote config status"),
                        );
                }
                error!(agent_id=%self.agent_id, err = %e, "Error building the supervisor");
                Err(SupervisorAssemblerError::AgentAssembleError(e.to_string()))
            }
            Ok(effective_agent) => {
                if let (Some(mut remote_status), Some(opamp_client)) =
                    (maybe_remote_status, maybe_opamp_client)
                {
                    // TODO: Do we need to check for the "applying" state?
                    // If we assembled successfully, we should apply and store the remote status in all cases no?
                    if remote_status.status_hash.is_applying() {
                        debug!(%self.agent_id, "applying remote config");
                        remote_status.status_hash.apply();

                        _ = self
                          .config_status_manager
                        .store_remote_status(&self.agent_id, &remote_status)
                        .inspect_err(
                            |e| error!(%self.agent_id, err = %e, "failed to store remote status"),
                        );

                        _ = opamp_client.update_effective_config().inspect_err(
                            |e| error!(%self.agent_id, %e, "effective config update failed"),
                        );

                        _ = OpampRemoteConfigStatus::Applied.report(opamp_client, &remote_status.status_hash).inspect_err(
                            |e| error!(%self.agent_id, %e, "error reporting remote config status"),
                        );
                    }
                    if let Some(err) = remote_status.status_hash.error_message() {
                        warn!(%self.agent_id, err = %err, "remote config failed. Building with previous stored config");
                        _ = OpampRemoteConfigStatus::Error(err).report(opamp_client, &remote_status.status_hash).inspect_err(|e| error!(%self.agent_id, %e, "error reporting remote config status"));
                    }
                }
                let supervisor = self
                    .supervisor_builder
                    .build_supervisor(effective_agent)
                    .map_err(|e| SupervisorAssemblerError::SupervisorBuildError(e.to_string()))?;

                Ok(supervisor)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::agent_control::config::{AgentID, AgentTypeFQN, SubAgentConfig};
    use crate::agent_type::environment::Environment;
    use crate::agent_type::runtime_config::{Deployment, OnHost, Runtime};
    use crate::opamp::callbacks::AgentCallbacks;
    use crate::opamp::client_builder::tests::MockStartedOpAMPClientMock;
    use crate::opamp::effective_config::loader::tests::MockEffectiveConfigLoaderMock;
    use crate::opamp::remote_config::hash::Hash;
    use crate::opamp::remote_config::status::AgentRemoteConfigStatus;
    use crate::opamp::remote_config::status_manager::tests::MockConfigStatusManagerMock;
    use crate::sub_agent::effective_agents_assembler::tests::MockEffectiveAgentAssemblerMock;
    use crate::sub_agent::effective_agents_assembler::{
        EffectiveAgent, EffectiveAgentsAssemblerError,
    };
    use crate::sub_agent::supervisor::assembler::SupervisorAssembler;
    use crate::sub_agent::supervisor::builder::tests::MockSupervisorBuilder;
    use crate::sub_agent::supervisor::starter::tests::MockSupervisorStarter;
    use crate::sub_agent::supervisor::stopper::tests::MockSupervisorStopper;
    use opamp_client::opamp::proto::RemoteConfigStatus;
    use opamp_client::opamp::proto::RemoteConfigStatuses::{Applied, Failed};
    use predicates::prelude::predicate;
    use std::sync::Arc;

    //Follow the same approach as before the refactor
    type AssemblerForTesting = SupervisorAssembler<
        MockConfigStatusManagerMock,
        MockSupervisorBuilder<MockSupervisorStarter>,
        MockEffectiveAgentAssemblerMock,
    >;

    type OpampClientForTest =
        MockStartedOpAMPClientMock<AgentCallbacks<MockEffectiveConfigLoaderMock>>;

    impl Default for AssemblerForTesting {
        fn default() -> Self {
            let agent_id = AgentID::new("some-agent-id").unwrap();
            let agent_cfg = SubAgentConfig {
                agent_type: AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
            };

            let effective_agent = final_agent(agent_id.clone(), agent_cfg.agent_type.clone());
            let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
            effective_agent_assembler.should_assemble_agent(
                &agent_id,
                &agent_cfg,
                &Environment::OnHost,
                None,
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

            SupervisorAssembler::new(
                Arc::new(MockConfigStatusManagerMock::new()),
                supervisor_builder,
                agent_id.clone(),
                agent_cfg.clone(),
                Arc::new(effective_agent_assembler),
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
        //  create a default assembler
        let mut assembler = AssemblerForTesting::default();

        // Modify expectations for this test
        // Expected calls on the hash repository
        let hash = Hash::new("some_hash".to_string());
        let mut applied_hash = hash.clone();
        applied_hash.apply();

        // Expected calls on the opamp client
        let mut started_opamp_client = OpampClientForTest::new();

        started_opamp_client.should_set_remote_config_status(RemoteConfigStatus {
            last_remote_config_hash: "some_hash".as_bytes().to_vec(),
            status: Applied as i32,
            error_message: "".to_string(),
        });

        started_opamp_client.should_update_effective_config(1);
        let maybe_opamp_client = Some(started_opamp_client);

        let remote_status = AgentRemoteConfigStatus {
            status_hash: hash,
            remote_config: None,
        };
        let mut applied_remote_status = remote_status.clone();
        applied_remote_status.status_hash.apply();

        let mut config_status_manager = MockConfigStatusManagerMock::new();
        config_status_manager
            .expect_store_remote_status()
            .with(
                predicate::eq(assembler.agent_id.clone()),
                predicate::eq(applied_remote_status),
            )
            .return_once(|_, _| Ok(()));

        assembler.config_status_manager = Arc::new(config_status_manager);

        assert!(assembler
            .assemble_supervisor(&maybe_opamp_client, Some(remote_status))
            .is_ok());
    }

    /// `maybe_opamp_client == Some(_)`
    /// `hash_repository.get(agent_id) fails` must not be different from the `None` cases, but we test it anyway to detect if this invariant changes
    /// `effective_agent_res == Ok(_)`
    #[test]
    fn test_assemble_supervisor_from_err_hash_ok_eff_agent() {
        //  create a default assembler
        let assembler = AssemblerForTesting::default();

        // Expected calls on the opamp client
        let maybe_opamp_client = Some(OpampClientForTest::new());

        assert!(assembler
            .assemble_supervisor(&maybe_opamp_client, None)
            .is_ok());
    }

    /// `maybe_opamp_client == Some(_)`
    /// `hash_repository.get(agent_id)? == Some(_)`
    /// `effective_agent_res == Err(_)`
    #[test]
    fn test_assemble_supervisor_from_some_hash_err_eff_agent() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_cfg = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        };

        let mut hash = Hash::new("some_hash".to_string());
        hash.fail("error assembling agents: `a random error happened!`".to_string());

        let expected_remote_config_status = RemoteConfigStatus {
            last_remote_config_hash: hash.get().as_bytes().to_vec(),
            status: Failed as i32,
            error_message: hash.error_message().unwrap(),
        };

        let effective_agent = final_agent(agent_id.clone(), agent_cfg.agent_type.clone());

        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
        effective_agent_assembler
            .expect_assemble_agent()
            .once()
            .returning(|_, _, _, _| {
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

        let remote_status = AgentRemoteConfigStatus {
            status_hash: hash,
            remote_config: None,
        };

        let mut config_status_manager = MockConfigStatusManagerMock::new();
        config_status_manager
            .expect_store_remote_status()
            .with(
                predicate::eq(agent_id.clone()),
                predicate::eq(remote_status.clone()),
            )
            .return_once(|_, _| Ok(()));

        let supervisor_assembler = SupervisorAssembler::new(
            Arc::new(config_status_manager),
            supervisor_builder,
            agent_id.clone(),
            agent_cfg.clone(),
            Arc::new(effective_agent_assembler),
            Environment::OnHost,
        );

        let maybe_opamp_client = Some(opamp_client);

        assert!(supervisor_assembler
            .assemble_supervisor(&maybe_opamp_client, Some(remote_status))
            .is_err());
    }

    /// `maybe_opamp_client == Some(_)`
    /// `hash_repository.get(agent_id)? == None`
    /// `effective_agent_res == Ok(_)`
    #[test]
    fn test_assemble_supervisor_from_none_hash_ok_eff_agent() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_cfg = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        };

        let effective_agent = final_agent(agent_id.clone(), agent_cfg.agent_type.clone());
        let assembled_effective_agent = effective_agent.clone();

        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
        effective_agent_assembler
            .expect_assemble_agent()
            .once()
            .return_once(move |_, _, _, _| Ok(assembled_effective_agent));

        let mut supervisor_builder = MockSupervisorBuilder::new();
        supervisor_builder
            .expect_build_supervisor()
            .with(predicate::function(move |e: &EffectiveAgent| {
                e == &effective_agent
            }))
            .return_once(|_| Ok(MockSupervisorStarter::new()));

        let opamp_client = OpampClientForTest::new();

        let supervisor_assembler = SupervisorAssembler::new(
            Arc::new(MockConfigStatusManagerMock::new()),
            supervisor_builder,
            agent_id.clone(),
            agent_cfg.clone(),
            Arc::new(effective_agent_assembler),
            Environment::OnHost,
        );

        let maybe_opamp_client = Some(opamp_client);

        assert!(supervisor_assembler
            .assemble_supervisor(&maybe_opamp_client, None)
            .is_ok());
    }

    /// `maybe_opamp_client == Some(_)`
    /// `hash_repository.get(agent_id)? == None`
    /// `effective_agent_res == Err(_)`
    #[test]
    fn test_assemble_supervisor_from_none_hash_err_eff_agent() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_cfg = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        };

        let effective_agent = final_agent(agent_id.clone(), agent_cfg.agent_type.clone());

        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
        effective_agent_assembler
            .expect_assemble_agent()
            .once()
            .returning(|_, _, _, _| {
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

        let supervisor_assembler = SupervisorAssembler::new(
            Arc::new(MockConfigStatusManagerMock::new()),
            supervisor_builder,
            agent_id.clone(),
            agent_cfg.clone(),
            Arc::new(effective_agent_assembler),
            Environment::OnHost,
        );

        let maybe_opamp_client = Some(opamp_client);

        assert!(supervisor_assembler
            .assemble_supervisor(&maybe_opamp_client, None)
            .is_err());
    }

    /// `maybe_opamp_client == None`
    /// `hash_repository.get(agent_id)? == Some(_)
    /// `effective_agent_res == Ok(_)`
    #[test]
    fn test_assemble_supervisor_from_ok_eff_agent_no_opamp() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_cfg = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        };

        // let hash = Hash::new("some_hash".to_string());

        let effective_agent = final_agent(agent_id.clone(), agent_cfg.agent_type.clone());
        let assembled_effective_agent = effective_agent.clone();

        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
        effective_agent_assembler
            .expect_assemble_agent()
            .once()
            .return_once(move |_, _, _, _| Ok(assembled_effective_agent));

        let mut supervisor_builder = MockSupervisorBuilder::new();
        supervisor_builder
            .expect_build_supervisor()
            .with(predicate::function(move |e: &EffectiveAgent| {
                e == &effective_agent
            }))
            .return_once(|_| Ok(MockSupervisorStarter::new()));

        let supervisor_assembler = SupervisorAssembler::new(
            Arc::new(MockConfigStatusManagerMock::new()),
            supervisor_builder,
            agent_id.clone(),
            agent_cfg.clone(),
            Arc::new(effective_agent_assembler),
            Environment::OnHost,
        );

        let maybe_opamp_client: Option<OpampClientForTest> = None;

        assert!(supervisor_assembler
            .assemble_supervisor(&maybe_opamp_client, None)
            .is_ok());
    }

    /// `maybe_opamp_client == None`
    /// `hash_repository.get(agent_id)? == None
    /// `effective_agent_res == Ok(_)`
    #[test]
    fn test_assemble_supervisor_from_ok_eff_agent_no_opamp_no_hash() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_cfg = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        };

        let effective_agent = final_agent(agent_id.clone(), agent_cfg.agent_type.clone());
        let assembled_effective_agent = effective_agent.clone();

        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
        effective_agent_assembler
            .expect_assemble_agent()
            .once()
            .return_once(move |_, _, _, _| Ok(assembled_effective_agent));

        let mut supervisor_builder = MockSupervisorBuilder::new();
        supervisor_builder
            .expect_build_supervisor()
            .with(predicate::function(move |e: &EffectiveAgent| {
                e == &effective_agent
            }))
            .return_once(|_| Ok(MockSupervisorStarter::new()));

        let supervisor_assembler = SupervisorAssembler::new(
            Arc::new(MockConfigStatusManagerMock::new()),
            supervisor_builder,
            agent_id.clone(),
            agent_cfg.clone(),
            Arc::new(effective_agent_assembler),
            Environment::OnHost,
        );

        let maybe_opamp_client: Option<OpampClientForTest> = None;

        assert!(supervisor_assembler
            .assemble_supervisor(&maybe_opamp_client, None)
            .is_ok());
    }

    /// `maybe_opamp_client == None`
    /// `hash_repository.get(agent_id)? == Some(_)
    /// `effective_agent_res == Err(_)`
    #[test]
    fn test_assemble_supervisor_from_err_eff_agent_no_opamp() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_cfg = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        };

        // let hash = Hash::new("some_hash".to_string());

        let effective_agent = final_agent(agent_id.clone(), agent_cfg.agent_type.clone());

        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
        effective_agent_assembler
            .expect_assemble_agent()
            .once()
            .return_once(move |_, _, _, _| {
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

        let supervisor_assembler = SupervisorAssembler::new(
            Arc::new(MockConfigStatusManagerMock::new()),
            supervisor_builder,
            agent_id.clone(),
            agent_cfg.clone(),
            Arc::new(effective_agent_assembler),
            Environment::OnHost,
        );

        let maybe_opamp_client: Option<OpampClientForTest> = None;

        assert!(supervisor_assembler
            .assemble_supervisor(&maybe_opamp_client, None)
            .is_err());
    }

    /// `maybe_opamp_client == None`
    /// `hash_repository.get(agent_id)? == None
    /// `effective_agent_res == Err(_)`
    #[test]
    fn test_assemble_supervisor_from_err_eff_agent_no_opamp_no_hash() {
        let agent_id = AgentID::new("some-agent-id").unwrap();
        let agent_cfg = SubAgentConfig {
            agent_type: AgentTypeFQN::try_from("namespace/some-agent-type:0.0.1").unwrap(),
        };

        let effective_agent = final_agent(agent_id.clone(), agent_cfg.agent_type.clone());

        let mut effective_agent_assembler = MockEffectiveAgentAssemblerMock::new();
        effective_agent_assembler
            .expect_assemble_agent()
            .once()
            .return_once(move |_, _, _, _| {
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

        let supervisor_assembler = SupervisorAssembler::new(
            Arc::new(MockConfigStatusManagerMock::new()),
            supervisor_builder,
            agent_id.clone(),
            agent_cfg.clone(),
            Arc::new(effective_agent_assembler),
            Environment::OnHost,
        );

        let maybe_opamp_client: Option<OpampClientForTest> = None;

        assert!(supervisor_assembler
            .assemble_supervisor(&maybe_opamp_client, None)
            .is_err());
    }

    fn final_agent(agent_id: AgentID, agent_fqn: AgentTypeFQN) -> EffectiveAgent {
        EffectiveAgent::new(
            agent_id,
            agent_fqn,
            Runtime {
                deployment: Deployment {
                    on_host: Some(OnHost::default()),
                    k8s: None,
                },
            },
        )
    }
}
