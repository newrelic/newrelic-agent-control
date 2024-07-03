use super::error::SubAgentBuilderError;
use crate::event::channel::EventPublisher;
use crate::event::SubAgentEvent;
use crate::opamp::callbacks::AgentCallbacks;
use crate::opamp::client_builder::OpAMPClientBuilder;
use crate::opamp::effective_config::loader::EffectiveConfigLoader;
use crate::opamp::hash_repository::HashRepository;
use crate::opamp::remote_config_report::report_remote_config_status_applied;
use crate::opamp::remote_config_report::report_remote_config_status_error;
use crate::sub_agent::effective_agents_assembler::EffectiveAgent;
use crate::sub_agent::effective_agents_assembler::EffectiveAgentsAssemblerError;
use crate::sub_agent::error;
use crate::sub_agent::error::SubAgentError;
use crate::sub_agent::event_processor::SubAgentEventProcessor;
use crate::super_agent::config::{AgentID, AgentTypeFQN, SubAgentConfig};
use std::sync::Arc;
use std::thread::JoinHandle;
use tracing::{debug, error, warn};

pub(crate) type SubAgentCallbacks<C> = AgentCallbacks<C>;

/// NotStartedSubAgent exposes a run method that starts processing events and, if present, the supervisors.
pub trait NotStartedSubAgent {
    type StartedSubAgent: StartedSubAgent;
    /// The run method (non-blocking) starts processing events and, if present, the supervisors.
    /// It returns a StartedSubAgent exposing .stop() to manage the running process.
    fn run(self) -> Self::StartedSubAgent;
}

/// The StartedSubAgent trait defines the interface for a supervisor that is already running.
/// Exposes information about the Sub Agent and a stop method that will stop the
/// supervised processes' execution and the loop processing the events.
pub trait StartedSubAgent {
    /// Returns the AgentID of the SubAgent
    fn agent_id(&self) -> AgentID;
    /// Returns the AgentType of the SubAgent
    fn agent_type(&self) -> AgentTypeFQN;
    /// Cancels the supervised process and returns its inner handle.
    fn stop(self) -> Result<Vec<JoinHandle<()>>, error::SubAgentError>;
}

pub trait SubAgentBuilder {
    type NotStartedSubAgent: NotStartedSubAgent;
    fn build(
        &self,
        agent_id: AgentID,
        sub_agent_config: &SubAgentConfig,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
    ) -> Result<Self::NotStartedSubAgent, error::SubAgentBuilderError>;
}

pub(crate) fn build_supervisor_or_default<HR, O, T, F, C>(
    agent_id: &AgentID,
    hash_repository: &Arc<HR>,
    maybe_opamp_client: &Option<O::Client>,
    effective_agent_res: Result<EffectiveAgent, EffectiveAgentsAssemblerError>,
    supervisor_builder_fn: F,
) -> Result<T, SubAgentBuilderError>
where
    HR: HashRepository,
    C: EffectiveConfigLoader + Send + Sync,
    O: OpAMPClientBuilder<SubAgentCallbacks<C>>,
    T: Default,
    F: FnOnce(EffectiveAgent) -> Result<T, SubAgentBuilderError>,
{
    // A sub-agent's supervisor can be started without a valid effective agent when an OpAMP
    // client is available. This is useful when the agent is in a failed state and the OpAMP
    // client is the only way to fix the configuration via remote configs.
    if let Some(opamp_client) = maybe_opamp_client {
        match (hash_repository.get(agent_id)?, effective_agent_res) {
            (Some(mut hash), Ok(effective_agent)) => {
                if hash.is_applying() {
                    debug!(%agent_id, "applying remote config");
                    hash.apply();
                    hash_repository.save(agent_id, &hash)?;
                    report_remote_config_status_applied(opamp_client, &hash)?;
                }

                if let Some(err_msg) = hash.error_message() {
                    warn!(%agent_id, err = %err_msg, "remote config failed. Building with previous stored config");
                    report_remote_config_status_error(opamp_client, &hash, err_msg)?;
                }

                supervisor_builder_fn(effective_agent)
            }
            (Some(mut hash), Err(err)) => {
                if !hash.is_failed() {
                    hash.fail(err.to_string());
                    hash_repository.save(agent_id, &hash)?;
                }

                report_remote_config_status_error(opamp_client, &hash, err.to_string())?;
                error!(%agent_id, %err, "failed to assemble agent from remote config");
                Ok(Default::default())
            }
            (None, Err(err)) => {
                debug!(%agent_id, "no previous remote config found");
                warn!(%agent_id, %err, "no previous config found. Failed to assemble agent from local or remote config");
                Ok(Default::default())
            }
            (None, Ok(effective_agent)) => {
                debug!(%agent_id, "no previous remote config found");
                supervisor_builder_fn(effective_agent)
            }
        }
    } else {
        supervisor_builder_fn(effective_agent_res?)
    }
}

////////////////////////////////////////////////////////////////////////////////////
// States for Started/Not Started Sub Agents
////////////////////////////////////////////////////////////////////////////////////
pub struct NotStarted<E>
where
    E: SubAgentEventProcessor,
{
    pub(crate) event_processor: E,
}

pub struct Started {
    pub(crate) event_loop_handle: JoinHandle<Result<(), SubAgentError>>,
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::{
        agent_type::runtime_config::Runtime,
        opamp::{
            client_builder::test::{MockOpAMPClientBuilderMock, MockStartedOpAMPClientMock},
            effective_config::loader::tests::MockEffectiveConfigLoaderMock,
            hash_repository::repository::test::MockHashRepositoryMock,
            remote_config_hash::Hash,
        },
    };
    use mockall::{mock, predicate, Sequence};
    use opamp_client::opamp::proto::{RemoteConfigStatus, RemoteConfigStatuses};

    mock! {
        pub StartedSubAgent {}

        impl StartedSubAgent for StartedSubAgent {
            fn stop(self) -> Result<Vec<JoinHandle<()>>, error::SubAgentError>;
            fn agent_id(&self) -> AgentID;
            fn agent_type(&self) -> AgentTypeFQN;
        }
    }

    impl MockStartedSubAgent {
        pub fn should_stop(&mut self) {
            self.expect_stop().once().returning(|| Ok(Vec::new()));
        }

        pub fn should_agent_id(&mut self, agent_id: AgentID) {
            self.expect_agent_id().once().return_once(|| agent_id);
        }

        pub fn should_agent_type(&mut self, agent_type_fqn: AgentTypeFQN) {
            self.expect_agent_type()
                .once()
                .return_once(|| agent_type_fqn);
        }
    }

    mock! {
        pub NotStartedSubAgent {}

        impl NotStartedSubAgent for NotStartedSubAgent {
            type StartedSubAgent = MockStartedSubAgent;

            fn run(self) -> <Self as NotStartedSubAgent>::StartedSubAgent;
        }
    }

    impl MockNotStartedSubAgent {
        pub fn should_run(&mut self, started_sub_agent: MockStartedSubAgent) {
            self.expect_run()
                .once()
                .return_once(move || started_sub_agent);
        }
    }

    mock! {
        pub SubAgentBuilderMock {}

        impl SubAgentBuilder for SubAgentBuilderMock {
            type NotStartedSubAgent = MockNotStartedSubAgent;

            fn build(
                &self,
                agent_id: AgentID,
                sub_agent_config: &SubAgentConfig,
                sub_agent_publisher: EventPublisher<SubAgentEvent>,
            ) -> Result<<Self as SubAgentBuilder>::NotStartedSubAgent, error::SubAgentBuilderError>;
        }
    }

    impl MockSubAgentBuilderMock {
        // should_build provides a helper method to create a subagent which runs and stops
        // successfully
        pub(crate) fn should_build(&mut self, times: usize) {
            self.expect_build().times(times).returning(|_, _, _| {
                let mut not_started_sub_agent = MockNotStartedSubAgent::new();
                not_started_sub_agent.expect_run().times(1).returning(|| {
                    let mut started_agent = MockStartedSubAgent::new();
                    started_agent
                        .expect_stop()
                        .times(1)
                        .returning(|| Ok(Vec::new()));
                    started_agent
                });
                Ok(not_started_sub_agent)
            });
        }

        pub(crate) fn should_build_not_started(
            &mut self,
            agent_id: &AgentID,
            sub_agent_config: SubAgentConfig,
            sub_agent: MockNotStartedSubAgent,
        ) {
            self.expect_build()
                .once()
                .with(
                    predicate::eq(agent_id.clone()),
                    predicate::eq(sub_agent_config),
                    predicate::always(),
                )
                .return_once(move |_, _, _| Ok(sub_agent));
        }
    }

    // Tests for `build_supervisor_or_default``
    // Essentially, the function `build_supervisor_or_default` defines the behavior for
    // a certain combination of the following parameters:
    //
    // - `maybe_opamp_client`, the presence of an OpAMP client. Can be either `Some(opamp_client)` or `None`.
    // - `hash_repository`, the presence of a hash in the hash repository for the given agent_id: The call to `hash_repository.get(agent_id)?` done inside the function returns either `Some(Hash)` or `None`.
    // - `effective_agent_res`, the result of the agent assembly attempt. Can be either `Ok(EffectiveAgent)` or `Err(EffectiveAgentsAssemblerError)`.
    //
    // When `maybe_opamp_client == None` the function `hash_repository.get(agent_id)?` won't be called, there's no value to check for.
    // We are safe to discard those from the testing set and only look at `effective_agent_res` in this case.
    //
    // So, we cover all cases.

    /// `maybe_opamp_client == Some(_)`
    /// `hash_repository.get(agent_id)? == Some(_)`
    /// `effective_agent_res == Ok(_)`
    #[test]
    fn test_build_supervisor_from_some_hash_ok_eff_agent() {
        let agent_id = AgentID::new("test-agent").unwrap();
        let effective_agent = Ok(EffectiveAgent::new(agent_id.clone(), Runtime::default()));

        // Expected calls on the hash repository
        let mut hash_repository = MockHashRepositoryMock::new();
        let mut seq = Sequence::new();
        hash_repository
            .expect_get()
            .once()
            .in_sequence(&mut seq)
            .returning(|_| Ok(Some(Hash::new("some_hash".to_string()))));
        hash_repository
            .expect_save()
            .once()
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));

        // Expected calls on the opamp client
        let mut started_opamp_client = MockStartedOpAMPClientMock::new();
        started_opamp_client
            .expect_set_remote_config_status()
            .once()
            .with(predicate::eq(RemoteConfigStatus {
                last_remote_config_hash: "some_hash".as_bytes().to_vec(),
                status: RemoteConfigStatuses::Applied as i32,
                error_message: "".to_string(),
            }))
            .returning(|_| Ok(()));

        // Actual test
        let actual = build_supervisor_or_default::<
            MockHashRepositoryMock,
            MockOpAMPClientBuilderMock<SubAgentCallbacks<MockEffectiveConfigLoaderMock>>,
            _,
            _,
            _,
        >(
            &agent_id,
            &Arc::new(hash_repository),
            &Some(started_opamp_client),
            effective_agent,
            |effective_agent| {
                Ok(assert_eq!(
                    EffectiveAgent::new(agent_id.clone(), Runtime::default()),
                    effective_agent
                ))
            },
        );

        assert!(actual.is_ok());
    }

    /// `maybe_opamp_client == Some(_)`
    /// `hash_repository.get(agent_id)? == Some(_)`
    /// `effective_agent_res == Err(_)`
    #[test]
    fn test_build_supervisor_from_some_hash_err_eff_agent() {
        let agent_id = AgentID::new("test-agent").unwrap();
        let effective_agent_res = Err(EffectiveAgentsAssemblerError::SerdeYamlError(
            serde::de::Error::custom("some_error"),
        ));

        // Expected calls on the hash repository
        let mut hash_repository = MockHashRepositoryMock::new();
        let mut seq = Sequence::new();
        hash_repository
            .expect_get()
            .once()
            .in_sequence(&mut seq)
            .returning(|_| Ok(Some(Hash::new("some_hash".to_string()))));
        hash_repository
            .expect_save()
            .once()
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));

        // Expected calls on the opamp client
        let mut started_opamp_client = MockStartedOpAMPClientMock::new();
        started_opamp_client
            .expect_set_remote_config_status()
            .once()
            .with(predicate::eq(RemoteConfigStatus {
                last_remote_config_hash: "some_hash".as_bytes().to_vec(),
                status: RemoteConfigStatuses::Failed as i32,
                error_message: "error assembling agents: `some_error`".to_string(),
            }))
            .returning(|_| Ok(()));

        // Actual test
        let actual = build_supervisor_or_default::<
            MockHashRepositoryMock,
            MockOpAMPClientBuilderMock<SubAgentCallbacks<MockEffectiveConfigLoaderMock>>,
            _,
            _,
            _,
        >(
            &agent_id,
            &Arc::new(hash_repository),
            &Some(started_opamp_client),
            effective_agent_res,
            |_| Ok(Some(())), // On error, we don't actually call this function and should be using the default for the Option<()> which is None, note we test this below!
        );

        assert!(actual.is_ok());
        assert!(actual.unwrap().is_none());
    }

    /// `maybe_opamp_client == Some(_)`
    /// `hash_repository.get(agent_id)? == None`
    /// `effective_agent_res == Ok(_)`
    #[test]
    fn test_build_supervisor_from_none_hash_ok_eff_agent() {
        let agent_id = AgentID::new("test-agent").unwrap();
        let effective_agent_res = Ok(EffectiveAgent::new(agent_id.clone(), Runtime::default()));

        // Expected calls on the hash repository
        let mut hash_repository = MockHashRepositoryMock::new();
        hash_repository.expect_get().once().returning(|_| Ok(None));

        // Expected calls on the opamp client
        let mut started_opamp_client = MockStartedOpAMPClientMock::new();
        started_opamp_client
            .expect_set_remote_config_status()
            .never();

        // Actual test
        let actual = build_supervisor_or_default::<
            MockHashRepositoryMock,
            MockOpAMPClientBuilderMock<SubAgentCallbacks<MockEffectiveConfigLoaderMock>>,
            _,
            _,
            _,
        >(
            &agent_id,
            &Arc::new(hash_repository),
            &Some(started_opamp_client),
            effective_agent_res,
            |effective_agent| {
                Ok(assert_eq!(
                    EffectiveAgent::new(agent_id.clone(), Runtime::default()),
                    effective_agent
                ))
            },
        );

        assert!(actual.is_ok());
    }

    /// `maybe_opamp_client == Some(_)`
    /// `hash_repository.get(agent_id)? == None`
    /// `effective_agent_res == Err(_)`
    #[test]
    fn test_build_supervisor_from_none_hash_err_eff_agent() {
        let agent_id = AgentID::new("test-agent").unwrap();
        let effective_agent_res = Err(EffectiveAgentsAssemblerError::SerdeYamlError(
            serde::de::Error::custom("some_error"),
        ));

        // Expected calls on the hash repository
        let mut hash_repository = MockHashRepositoryMock::new();
        hash_repository.expect_get().once().returning(|_| Ok(None));

        // Expected calls on the opamp client
        let mut started_opamp_client = MockStartedOpAMPClientMock::new();
        started_opamp_client
            .expect_set_remote_config_status()
            .never();

        // Actual test
        let actual = build_supervisor_or_default::<
            MockHashRepositoryMock,
            MockOpAMPClientBuilderMock<SubAgentCallbacks<MockEffectiveConfigLoaderMock>>,
            _,
            _,
            _,
        >(
            &agent_id,
            &Arc::new(hash_repository),
            &Some(started_opamp_client),
            effective_agent_res,
            |_| Ok(Some(())), // On error, we don't actually call this function and should be using the default for the Option<()> which is None, note we test this below!
        );

        assert!(actual.is_ok());
        assert!(actual.unwrap().is_none());
    }

    /// `maybe_opamp_client == None`
    /// `hash_repository.get(agent_id)? == Some(_) || hash_repository.get(agent_id)? == None` (it won't be called)
    /// `effective_agent_res == Ok(_)`
    #[test]
    fn test_build_supervisor_from_ok_eff_agent_no_opamp() {
        let agent_id = AgentID::new("test-agent").unwrap();
        let effective_agent_res = Ok(EffectiveAgent::new(agent_id.clone(), Runtime::default()));

        // Expected calls on the hash repository
        let mut hash_repository = MockHashRepositoryMock::new();
        hash_repository.expect_get().never();

        // Actual test
        let actual = build_supervisor_or_default::<
            MockHashRepositoryMock,
            MockOpAMPClientBuilderMock<SubAgentCallbacks<MockEffectiveConfigLoaderMock>>,
            _,
            _,
            _,
        >(
            &agent_id,
            &Arc::new(hash_repository),
            &None,
            effective_agent_res,
            |effective_agent| {
                Ok(assert_eq!(
                    EffectiveAgent::new(agent_id.clone(), Runtime::default()),
                    effective_agent
                ))
            },
        );

        assert!(actual.is_ok());
    }

    /// `maybe_opamp_client == None`
    /// `hash_repository.get(agent_id)? == Some(_) || hash_repository.get(agent_id)? == None` (it won't be called)
    /// `effective_agent_res == Err(_)`
    #[test]
    fn test_build_supervisor_from_err_eff_agent_no_opamp() {
        let agent_id = AgentID::new("test-agent").unwrap();
        let effective_agent_res = Err(EffectiveAgentsAssemblerError::SerdeYamlError(
            serde::de::Error::custom("some_error"),
        ));

        // Expected calls on the hash repository
        let mut hash_repository = MockHashRepositoryMock::new();
        hash_repository.expect_get().never();

        // Actual test
        let actual = build_supervisor_or_default::<
            MockHashRepositoryMock,
            MockOpAMPClientBuilderMock<SubAgentCallbacks<MockEffectiveConfigLoaderMock>>,
            _,
            _,
            _,
        >(
            &agent_id,
            &Arc::new(hash_repository),
            &None,
            effective_agent_res,
            |_| Ok(Some(())), // On error, we don't actually call this function, this time, the call to `build_supervisor_or_default` will bubble up the error!
        );

        assert!(actual.is_err());
    }
}
