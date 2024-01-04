use crate::config::agent_type::agent_types::FinalAgent;
use crate::config::agent_values::AgentValues;
use crate::config::error::SuperAgentConfigError;
use crate::config::store::{
    SubAgentsConfigDeleter, SubAgentsConfigLoader, SubAgentsConfigStorer, SuperAgentConfigStoreFile,
};
use crate::config::super_agent_configs::{AgentID, AgentTypeFQN, SubAgentConfig, SubAgentsConfig};
use crate::context::Context;
use crate::fs::directory_manager::DirectoryManagerFs;
use crate::opamp::callbacks::AgentCallbacks;
use crate::opamp::remote_config::{RemoteConfig, RemoteConfigError};
use crate::opamp::remote_config_hash::{Hash, HashRepository, HashRepositoryFile};
use crate::opamp::remote_config_report::{
    report_remote_config_status_applied, report_remote_config_status_applying,
    report_remote_config_status_error,
};
use crate::sub_agent::collection::{NotStartedSubAgents, StartedSubAgents};
use crate::sub_agent::error::SubAgentBuilderError;
use crate::sub_agent::logger::{AgentLog, EventLogger, StdEventReceiver};
use crate::sub_agent::SubAgentBuilder;

use crate::event::event::{Event, OpAMPEvent, SubAgentEvent, SuperAgentEvent};
use crate::sub_agent::values::values_repository::{ValuesRepository, ValuesRepositoryFile};
use crate::sub_agent::NotStartedSubAgent;
use crate::super_agent::defaults::{SUPER_AGENT_NAMESPACE, SUPER_AGENT_TYPE, SUPER_AGENT_VERSION};
use crate::super_agent::error::AgentError;
use crate::super_agent::super_agent::EffectiveAgentsError::{
    EffectiveAgentExists, EffectiveAgentNotFound,
};
use futures::executor::block_on;
use opamp_client::opamp::proto::{RemoteConfigStatus, RemoteConfigStatuses};
use opamp_client::StartedClient;
use std::collections::HashMap;
use std::string::ToString;
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use thiserror::Error;
use tracing::{error, info, warn};

use super::opamp::remote_config_publisher::SuperAgentRemoteConfigPublisher;

pub(super) type SuperAgentCallbacks = AgentCallbacks<SuperAgentRemoteConfigPublisher>;

pub struct SuperAgent<
    'a,
    S,
    O,
    HR = HashRepositoryFile,
    SL = SuperAgentConfigStoreFile,
    HRS = HashRepositoryFile,
    VR = ValuesRepositoryFile<DirectoryManagerFs>,
> where
    O: StartedClient<SuperAgentCallbacks>,
    HR: HashRepository,
    SL: SubAgentsConfigStorer + SubAgentsConfigLoader + SubAgentsConfigDeleter,
    HRS: HashRepository,
    S: SubAgentBuilder,
    VR: ValuesRepository,
{
    opamp_client: Option<O>,
    sub_agent_builder: S,
    remote_config_hash_repository: &'a HR,
    agent_id: AgentID,
    sub_agent_remote_config_hash_repository: &'a HRS,
    remote_values_repo: VR,
    sub_agents_config_store: Arc<SL>,
}

impl<'a, S, O, HR, SL, HRS, VR> SuperAgent<'a, S, O, HR, SL, HRS, VR>
where
    O: StartedClient<SuperAgentCallbacks>,
    HR: HashRepository,
    S: SubAgentBuilder,
    SL: SubAgentsConfigStorer + SubAgentsConfigLoader + SubAgentsConfigDeleter,
    HRS: HashRepository,
    VR: ValuesRepository,
{
    pub fn new(
        opamp_client: Option<O>,
        remote_config_hash_repository: &'a HR,
        sub_agent_builder: S,
        sub_agents_config_store: Arc<SL>,
        sub_agent_remote_config_hash_repository: &'a HRS,
        values_repo: VR,
    ) -> Self {
        Self {
            opamp_client,
            remote_config_hash_repository,
            sub_agent_builder,
            // unwrap as we control content of the SUPER_AGENT_ID constant
            agent_id: AgentID::new_super_agent_id(),
            sub_agents_config_store,
            sub_agent_remote_config_hash_repository,
            remote_values_repo: values_repo,
        }
    }

    fn agent_id(&self) -> &AgentID {
        &self.agent_id
    }

    pub fn run(self, ctx: Context<Option<Event>>) -> Result<(), AgentError> {
        info!("Creating agent's communication channels");
        // Channel will be closed when tx is dropped and no reference to it is alive
        let (tx, rx) = mpsc::channel();

        let output_manager = StdEventReceiver::default().log(rx);

        if let Some(opamp_handle) = &self.opamp_client {
            // TODO should we error on first launch with no hash file?
            let remote_config_hash = self
                .remote_config_hash_repository
                .get(self.agent_id())
                .map_err(|e| {
                    warn!(
                        "OpAMP enabled but no previous remote configuration found: {}",
                        e
                    )
                })
                .ok();

            if let Some(mut hash) = remote_config_hash {
                if !hash.is_applied() {
                    report_remote_config_status_applied(opamp_handle, &hash)?;
                    self.set_config_hash_as_applied(&mut hash)?;
                }
            }
        } else {
            // Delete remote values
            self.remote_values_repo.delete_remote_all()?;
        }

        info!("Starting the supervisor group.");
        // let effective_agents = self.load_effective_agents(&self.sub_agents_config_store.load()?)?;
        let sub_agents_config = &self.sub_agents_config_store.load()?;
        let not_started_sub_agents = self.load_sub_agents(sub_agents_config, &tx, ctx.clone())?;

        // Run all the Sub Agents
        let running_sub_agents = not_started_sub_agents.run()?;
        self.process_events(ctx.clone(), running_sub_agents, tx, &self.opamp_client)?;

        if let Some(handle) = self.opamp_client {
            info!("Stopping and setting to unhealthy the OpAMP Client");
            let health = opamp_client::opamp::proto::AgentHealth {
                healthy: false,
                last_error: "".to_string(),
                start_time_unix_nano: 0,
            };
            block_on(handle.set_health(health))?;
            block_on(handle.stop())?;
        }

        info!("Waiting for the output manager to finish");
        output_manager.join().unwrap();

        info!("SuperAgent finished");
        Ok(())
    }

    fn set_config_hash_as_applied(&self, hash: &mut Hash) -> Result<(), AgentError> {
        hash.apply();
        self.remote_config_hash_repository
            .save(self.agent_id(), hash)?;

        Ok(())
    }

    // load_sub_agents returns a collection of not started sub agents given the corresponding
    // EffectiveAgents
    fn load_sub_agents(
        &self,
        sub_agents_config: &SubAgentsConfig,
        tx: &Sender<AgentLog>,
        ctx: Context<Option<Event>>,
    ) -> Result<NotStartedSubAgents<S::NotStartedSubAgent>, AgentError> {
        Ok(NotStartedSubAgents::from(
            sub_agents_config
                .agents
                .iter()
                .map(|(agent_id, sub_agent_config)| {
                    // FIXME: we force OK(agent) but we need to check also agent not assembled when
                    // on first stat because it can be a crash after a remote_config_change
                    let not_started_agent = self.sub_agent_builder.build(
                        agent_id.clone(),
                        sub_agent_config,
                        tx.clone(),
                        ctx.clone(),
                    )?;
                    Ok((agent_id.clone(), not_started_agent))
                })
                .collect::<Result<HashMap<_, _>, SubAgentBuilderError>>()?,
        ))
    }

    // Recreates a Sub Agent by its agent_id meaning:
    //  * Remove and stop the existing running Sub Agent from the Running Sub Agents
    //  * Recreate the Final Agent using the Agent Type and the latest persisted config
    //  * Build a Stopped Sub Agent
    //  * Run the Sub Agent and add it to the Running Sub Agents
    fn recreate_sub_agent(
        &self,
        agent_id: AgentID,
        sub_agent_config: &SubAgentConfig,
        tx: Sender<AgentLog>,
        running_sub_agents: &mut StartedSubAgents<
            <S::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
        ctx: Context<Option<Event>>,
    ) -> Result<(), AgentError> {
        running_sub_agents.stop_remove(&agent_id)?;

        self.create_sub_agent(agent_id, sub_agent_config, tx, running_sub_agents, ctx)
    }

    // runs and adds into the sub_agents collection the given agent
    fn create_sub_agent(
        &self,
        agent_id: AgentID,
        sub_agent_config: &SubAgentConfig,
        tx: Sender<AgentLog>,
        running_sub_agents: &mut StartedSubAgents<
            <S::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
        ctx: Context<Option<Event>>,
    ) -> Result<(), AgentError> {
        running_sub_agents.insert(
            agent_id.clone(),
            self.sub_agent_builder
                .build(agent_id, sub_agent_config, tx, ctx)?
                .run()?,
        );

        Ok(())
    }

    fn process_events(
        &self,
        ctx: Context<Option<Event>>,
        mut sub_agents: StartedSubAgents<
            <<S as SubAgentBuilder>::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
        tx: Sender<AgentLog>,
        maybe_opamp_client: &Option<O>,
    ) -> Result<(), AgentError> {
        loop {
            // blocking wait until context is woken up
            if let Some(event) = ctx.wait_condvar().unwrap() {
                match event {
                    Event::SuperAgentEvent(SuperAgentEvent::StopRequested) => {
                        drop(tx); //drop the main channel sender to stop listener
                        break sub_agents.stop()?;
                    }
                    Event::OpAMPEvent(OpAMPEvent::InvalidRemoteConfigReceived(
                        remote_config_error,
                    )) => {
                        if let Some(opamp_client) = &maybe_opamp_client {
                            self.process_super_agent_remote_config_error(
                                opamp_client,
                                remote_config_error,
                            )?;
                        } else {
                            unreachable!("got remote config without OpAMP being enabled")
                        }
                    }
                    Event::OpAMPEvent(OpAMPEvent::ValidRemoteConfigReceived(mut remote_config)) => {
                        // TODO This condition should be removed to subagent event loop
                        if !remote_config.agent_id.is_super_agent_id() {
                            self.process_sub_agent_remote_config(
                                remote_config,
                                &mut sub_agents,
                                tx.clone(),
                                ctx.clone(),
                            )?;
                            break;
                        }

                        if let Some(opamp_client) = &maybe_opamp_client {
                            self.process_super_agent_remote_config(
                                opamp_client,
                                &mut remote_config,
                                tx.clone(),
                                &mut sub_agents,
                                ctx.clone(),
                            )?;
                        } else {
                            unreachable!("got remote config without OpAMP being enabled")
                        }
                    }

                    // TODO: The SubAgentRemoteConfig events now are opamp events that need
                    // to be handled on the subagent event loop
                    Event::SubAgentEvent(SubAgentEvent::ConfigUpdated(agent_id)) => {
                        let config = self.sub_agents_config_store.load()?;
                        let config = config.get(&agent_id)?;
                        self.recreate_sub_agent(
                            agent_id,
                            config,
                            tx.clone(),
                            &mut sub_agents,
                            ctx.clone(),
                        )?;
                    }
                };
            }
            // spurious condvar wake up, loop should continue
        }
        Ok(())
    }

    // TODO This call should be moved to on subagent event loop when opamp event remote_config
    // Sub Agent on remote config
    fn process_sub_agent_remote_config(
        &self,
        mut remote_config: RemoteConfig,
        sub_agents: &mut StartedSubAgents<
            <S::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
        tx: Sender<AgentLog>,
        ctx: Context<Option<Event>>,
    ) -> Result<(), AgentError> {
        let agent_id = remote_config.agent_id.clone();

        self.sub_agent_remote_config_hash_repository
            .save(&remote_config.agent_id, &remote_config.hash)?;
        let remote_config_value = remote_config.get_unique()?;
        // If remote config is empty, we delete the persisted remote config so later the store
        // will load the local config
        if remote_config_value.is_empty() {
            self.remote_values_repo
                .delete_remote(&remote_config.agent_id)?;
        } else {
            // If the config is not valid log we cannot report it to OpAMP as
            // we don't have access to the Sub Agent OpAMP Client here (yet) so
            // for now we mark the remote config as failed and we don't persist it.
            // When the Sub Agent is "recreated" it will report the remote config
            // as failed.
            match AgentValues::try_from(remote_config_value.to_string()) {
                Err(e) => {
                    error!("Error applying Sub Agent remote config: {}", e);
                    remote_config.hash.fail(e.to_string());
                    self.sub_agent_remote_config_hash_repository
                        .save(&remote_config.agent_id, &remote_config.hash)?;
                }
                Ok(agent_values) => self
                    .remote_values_repo
                    .store_remote(&remote_config.agent_id, &agent_values)?,
            }
        }

        let config = self.sub_agents_config_store.load()?;
        let config = config.get(&agent_id)?;
        self.recreate_sub_agent(agent_id, config, tx.clone(), sub_agents, ctx)?;

        Ok(())
    }

    // Super Agent on remote config
    // Configuration will be reported as applying to OpAMP
    // Valid configuration will be applied and reported as applied to OpAMP
    // Invalid configuration will not be applied and therefore it will not break the execution
    // of the Super Agent. It will be logged and reported as failed to OpAMP
    fn process_super_agent_remote_config(
        &self,
        opamp_client: &O,
        remote_config: &mut RemoteConfig,
        tx: Sender<AgentLog>,
        running_sub_agents: &mut StartedSubAgents<
            <S::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
        ctx: Context<Option<Event>>,
    ) -> Result<(), AgentError> {
        info!("Applying SuperAgent remote config");
        report_remote_config_status_applying(opamp_client, &remote_config.hash)?;

        if let Err(err) =
            self.apply_remote_config(remote_config.clone(), tx, running_sub_agents, ctx)
        {
            let error_message = format!("Error applying Super Agent remote config: {}", err);
            error!(error_message);
            Ok(report_remote_config_status_error(
                opamp_client,
                &remote_config.hash,
                error_message,
            )?)
        } else {
            self.set_config_hash_as_applied(&mut remote_config.hash)?;
            Ok(report_remote_config_status_applied(
                opamp_client,
                &remote_config.hash,
            )?)
        }
    }

    // Super Agent on remote config
    fn process_super_agent_remote_config_error(
        &self,
        opamp_client: &O,
        remote_config_err: RemoteConfigError,
    ) -> Result<(), AgentError> {
        if let RemoteConfigError::InvalidConfig(hash, error) = remote_config_err {
            block_on(opamp_client.set_remote_config_status(RemoteConfigStatus {
                last_remote_config_hash: hash.into_bytes(),
                error_message: error,
                status: RemoteConfigStatuses::Failed as i32,
            }))?;
            Ok(())
        } else {
            unreachable!()
        }
    }

    // apply a remote config to the running sub agents
    fn apply_remote_config(
        &self,
        remote_config: RemoteConfig,
        tx: Sender<AgentLog>,
        running_sub_agents: &mut StartedSubAgents<
            <S::NotStartedSubAgent as NotStartedSubAgent>::StartedSubAgent,
        >,
        ctx: Context<Option<Event>>,
    ) -> Result<(), AgentError> {
        //TODO fix get_unique to fit OpAMP Spec of having a "" when single config
        let content = remote_config.get_unique()?;
        let old_sub_agents_config = self.sub_agents_config_store.load()?;

        let sub_agents_config = if !content.is_empty() {
            SubAgentsConfig::try_from(&remote_config)?
        } else {
            self.sub_agents_config_store.delete()?;
            self.sub_agents_config_store.load()?
        };

        // recreate from new configuration
        sub_agents_config
            .iter()
            .try_for_each(|(agent_id, agent_config)| {
                if let Ok(old_sub_agent_config) = old_sub_agents_config.get(agent_id) {
                    if old_sub_agent_config != agent_config {
                        // new config
                        info!("Recreating SubAgent {}", agent_id);
                        return self.recreate_sub_agent(
                            agent_id.clone(),
                            agent_config,
                            tx.clone(),
                            running_sub_agents,
                            ctx.clone(),
                        );
                    } else {
                        // no changes applied
                        return Ok(());
                    }
                }
                info!("Creating SubAgent {}", agent_id);
                self.create_sub_agent(
                    agent_id.clone(),
                    agent_config,
                    tx.clone(),
                    running_sub_agents,
                    ctx.clone(),
                )
            })?;

        // remove sub agents not used anymore
        old_sub_agents_config
            .iter()
            .try_for_each(|(agent_id, _agent_config)| {
                if let Err(SuperAgentConfigError::SubAgentNotFound(_)) =
                    sub_agents_config.get(agent_id)
                {
                    info!("Stopping SubAgent {}", agent_id);
                    return running_sub_agents.stop_remove(agent_id);
                }
                Ok(())
            })?;

        // TODO improve this code.
        if !content.is_empty() {
            self.sub_agents_config_store.store(&sub_agents_config)?;
        }
        //
        Ok(self
            .remote_config_hash_repository
            .save(self.agent_id(), &remote_config.hash)?)
    }
}

pub fn super_agent_fqn() -> AgentTypeFQN {
    AgentTypeFQN::from(
        format!(
            "{}/{}:{}",
            SUPER_AGENT_NAMESPACE, SUPER_AGENT_TYPE, SUPER_AGENT_VERSION
        )
        .as_str(),
    )
}

#[derive(Debug, Default, PartialEq, Clone)]
pub struct EffectiveAgents {
    pub agents: HashMap<AgentID, FinalAgent>,
}

#[derive(Error, Debug)]
pub enum EffectiveAgentsError {
    #[error("effective agent `{0}` not found")]
    EffectiveAgentNotFound(String),
    #[error("effective agent `{0}` already exists")]
    EffectiveAgentExists(String),
}

impl EffectiveAgents {
    pub fn get(&self, agent_id: &AgentID) -> Result<&FinalAgent, EffectiveAgentsError> {
        match self.agents.get(agent_id) {
            None => Err(EffectiveAgentNotFound(agent_id.to_string())),
            Some(agent) => Ok(agent),
        }
    }

    pub fn add(
        &mut self,
        agent_id: AgentID,
        agent: FinalAgent,
    ) -> Result<(), EffectiveAgentsError> {
        if self.get(&agent_id).is_ok() {
            return Err(EffectiveAgentExists(agent_id.to_string()));
        }
        self.agents.insert(agent_id, agent);
        Ok(())
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use crate::config::agent_type::trivial_value::TrivialValue;
    use crate::config::agent_values::AgentValues;
    use crate::config::store::tests::MockSubAgentsConfigStore;
    use crate::config::store::{
        SubAgentsConfigDeleter, SubAgentsConfigLoader, SubAgentsConfigStorer,
    };
    use crate::config::super_agent_configs::{
        AgentID, AgentTypeFQN, SubAgentConfig, SubAgentsConfig,
    };
    use crate::context::Context;
    use crate::event::event::{Event, OpAMPEvent, SubAgentEvent, SuperAgentEvent};
    use crate::opamp::client_builder::test::MockStartedOpAMPClientMock;
    use crate::opamp::remote_config::{ConfigMap, RemoteConfig};
    use crate::opamp::remote_config_hash::test::MockHashRepositoryMock;
    use crate::opamp::remote_config_hash::{Hash, HashRepository};
    use crate::sub_agent::collection::StartedSubAgents;
    use crate::sub_agent::test::MockStartedSubAgent;
    use crate::sub_agent::values::values_repository::test::MockRemoteValuesRepositoryMock;
    use crate::sub_agent::values::values_repository::ValuesRepository;
    use crate::sub_agent::{test::MockSubAgentBuilderMock, SubAgentBuilder};
    use crate::super_agent::super_agent::SuperAgent;
    use mockall::predicate;
    use opamp_client::opamp::proto::RemoteConfigStatus;
    use opamp_client::opamp::proto::RemoteConfigStatuses::{Applied, Applying, Failed};

    use opamp_client::StartedClient;
    use std::collections::HashMap;
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::thread::{sleep, spawn};
    use std::time::Duration;

    use super::SuperAgentCallbacks;

    ////////////////////////////////////////////////////////////////////////////////////
    // Custom Agent constructor for tests
    ////////////////////////////////////////////////////////////////////////////////////
    impl<'a, S, O, HR, SL, HRS, VR> SuperAgent<'a, S, O, HR, SL, HRS, VR>
    where
        O: StartedClient<SuperAgentCallbacks>,
        HR: HashRepository,
        S: SubAgentBuilder,
        SL: SubAgentsConfigStorer + SubAgentsConfigLoader + SubAgentsConfigDeleter,
        HRS: HashRepository,
        VR: ValuesRepository,
    {
        pub fn new_custom(
            opamp_client: Option<O>,
            remote_config_hash_repository: &'a HR,
            sub_agent_builder: S,
            sub_agents_config_store: SL,
            sub_agent_remote_config_hash_repository: &'a HRS,
            sub_agent_values_repo: VR,
        ) -> Self {
            SuperAgent {
                opamp_client,
                remote_config_hash_repository,
                sub_agent_builder,
                agent_id: AgentID::new_super_agent_id(),
                sub_agents_config_store: Arc::new(sub_agents_config_store),
                sub_agent_remote_config_hash_repository,
                remote_values_repo: sub_agent_values_repo,
            }
        }
    }

    #[test]
    fn run_and_stop_supervisors_no_agents() {
        let sub_agent_hash_repository_mock = MockHashRepositoryMock::new();
        let sub_agent_values_repo = MockRemoteValuesRepositoryMock::new();
        let mut sub_agents_config_store = MockSubAgentsConfigStore::new();
        let mut hash_repository_mock = MockHashRepositoryMock::new();
        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_health(1);
        started_client.should_stop(1);

        sub_agents_config_store
            .expect_load()
            .returning(|| Ok(HashMap::new().into()));

        hash_repository_mock.expect_get().times(1).returning(|_| {
            let mut hash = Hash::new("a-hash".to_string());
            hash.apply();
            Ok(hash)
        });

        // no agents in the supervisor group
        let agent = SuperAgent::new_custom(
            Some(started_client),
            &hash_repository_mock,
            MockSubAgentBuilderMock::new(),
            sub_agents_config_store,
            &sub_agent_hash_repository_mock,
            sub_agent_values_repo,
        );

        let ctx = Context::new();

        // stop all agents after 50 milliseconds
        send_event_after(
            ctx.clone(),
            SuperAgentEvent::StopRequested.into(),
            Duration::from_millis(50),
        );

        assert!(agent.run(ctx).is_ok())
    }

    #[test]
    fn run_and_stop_supervisors() {
        let sub_agent_hash_repository_mock = MockHashRepositoryMock::new();
        let sub_agent_values_repo = MockRemoteValuesRepositoryMock::new();
        let mut sub_agents_config_store = MockSubAgentsConfigStore::new();
        let mut hash_repository_mock = MockHashRepositoryMock::new();
        let mut sub_agent_builder = MockSubAgentBuilderMock::new();

        let sub_agents_config = sub_agents_default_config();

        // Super Agent OpAMP
        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_health(1);
        started_client.should_stop(1);

        hash_repository_mock.expect_get().times(1).returning(|_| {
            let mut hash = Hash::new("a-hash".to_string());
            hash.apply();
            Ok(hash)
        });

        // it should build two subagents: nrdot + infra_agent
        sub_agent_builder.should_build(2);

        sub_agents_config_store
            .expect_load()
            .returning(move || Ok(sub_agents_config.clone()));

        let agent = SuperAgent::new_custom(
            Some(started_client),
            &hash_repository_mock,
            sub_agent_builder,
            sub_agents_config_store,
            &sub_agent_hash_repository_mock,
            sub_agent_values_repo,
        );

        let ctx = Context::new();
        // stop all agents after 50 milliseconds
        send_event_after(
            ctx.clone(),
            SuperAgentEvent::StopRequested.into(),
            Duration::from_millis(50),
        );
        assert!(agent.run(ctx).is_ok())
    }

    #[test]
    fn receive_opamp_remote_config() {
        let sub_agent_hash_repository_mock = MockHashRepositoryMock::new();
        let sub_agent_values_repo = MockRemoteValuesRepositoryMock::new();
        let mut hash_repository_mock = MockHashRepositoryMock::new();
        let mut sub_agent_builder = MockSubAgentBuilderMock::new();

        // Super Agent OpAMP
        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_health(1);
        // applying and applied
        started_client
            .expect_set_remote_config_status()
            .times(2)
            .returning(|_| Ok(()));
        started_client.should_stop(1);

        let mut sub_agents_config_store = MockSubAgentsConfigStore::new();
        sub_agents_config_store
            .expect_load()
            .returning(|| Ok(sub_agents_default_config()));
        // updated agent
        sub_agents_config_store
            .expect_store()
            .once()
            .returning(|_| Ok(()));

        hash_repository_mock
            .expect_get()
            .with(predicate::eq(AgentID::new_super_agent_id()))
            .times(1)
            .returning(|_| {
                let mut hash = Hash::new("a-hash".to_string());
                hash.apply();
                Ok(hash)
            });

        hash_repository_mock
            .expect_save()
            .with(
                predicate::eq(AgentID::new_super_agent_id()),
                predicate::eq(Hash::new("a-hash".to_string())),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        hash_repository_mock
            .expect_save()
            .with(
                predicate::eq(AgentID::new_super_agent_id()),
                predicate::eq(Hash::applied("a-hash".to_string())),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        // it should build two subagents: nrdot + infra_agent
        sub_agent_builder.should_build(2);

        let ctx = Context::new();
        let running_agent = spawn({
            let ctx = ctx.clone();
            move || {
                // two agents in the supervisor group
                let agent = SuperAgent::new_custom(
                    Some(started_client),
                    &hash_repository_mock,
                    sub_agent_builder,
                    sub_agents_config_store,
                    &sub_agent_hash_repository_mock,
                    sub_agent_values_repo,
                );
                agent.run(ctx)
            }
        });

        let remote_config = RemoteConfig {
            agent_id: AgentID::new_super_agent_id(),
            hash: Hash::new("a-hash".to_string()),
            config_map: ConfigMap::new(HashMap::from([(
                "".to_string(),
                r#"
agents:
  infra_agent:
    agent_type: "newrelic/com.newrelic.infrastructure_agent:0.0.1"
"#
                .to_string(),
            )])),
        };

        // TODO: replace Context with a unbuffered channel?
        sleep(Duration::from_millis(100));
        ctx.cancel_all(Some(
            OpAMPEvent::ValidRemoteConfigReceived(remote_config).into(),
        ))
        .unwrap();
        sleep(Duration::from_millis(50));
        ctx.cancel_all(Some(SuperAgentEvent::StopRequested.into()))
            .unwrap();
        assert!(running_agent.join().is_ok())
    }

    // TODO Move to SubAgent when its event loop is created
    #[test]
    fn receive_sub_agent_opamp_remote_config_existing_sub_agent_should_be_recreated() {
        let ctx = Context::new();
        let (tx, _) = mpsc::channel();

        let hash_repository_mock = MockHashRepositoryMock::new();
        let mut sub_agent_builder = MockSubAgentBuilderMock::new();
        let mut sub_agents_config_store = MockSubAgentsConfigStore::new();
        let mut sub_agent_hash_repository_mock = MockHashRepositoryMock::new();
        let mut sub_agent_values_repo = MockRemoteValuesRepositoryMock::new();

        // Given that we have 3 running Sub Agents
        let mut sub_agents = StartedSubAgents::from(HashMap::from([
            (
                AgentID::new("fluent_bit").unwrap(),
                MockStartedSubAgent::new(),
            ),
            (
                AgentID::new("infra_agent").unwrap(),
                MockStartedSubAgent::new(),
            ),
            (AgentID::new("nrdot").unwrap(), MockStartedSubAgent::new()),
        ]));

        // When we receive a remote config for a Sub Agent
        let sub_agent_id = AgentID::new("infra_agent").unwrap();

        let remote_config = RemoteConfig {
            agent_id: sub_agent_id.clone(),
            hash: Hash::new("sub-agent-hash".to_string()),
            config_map: ConfigMap::new(HashMap::from([(
                "".to_string(),
                r#"
config_file: /some/path/newrelic-infra.yml
"#
                .to_string(),
            )])),
        };

        // Then hash repository should save the received hash
        sub_agent_hash_repository_mock
            .should_save_hash(&remote_config.agent_id, &remote_config.hash);
        // And values repo should store the received config as values
        let expected_agent_values = AgentValues::new(HashMap::from([(
            "config_file".to_string(),
            TrivialValue::String("/some/path/newrelic-infra.yml".to_string()),
        )]));
        sub_agent_values_repo.should_store_remote(&sub_agent_id, &expected_agent_values);
        // And we reload the config from the Sub Agent Config Store
        let sub_agents_config = SubAgentsConfig::from(HashMap::from([
            (
                AgentID::new("nrdot").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::from("fqn_rdot"),
                },
            ),
            (
                AgentID::new("infra_agent").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::from("fqn_infra_agent"),
                },
            ),
            (
                AgentID::new("fluent_bit").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::from("fqn_fluent_bit"),
                },
            ),
        ]));
        sub_agents_config_store.should_load(&sub_agents_config);
        // And the Sub Agent should be stopped
        sub_agents.get(&sub_agent_id).should_stop();
        // And the Sub Agent should be re-created
        sub_agent_builder.should_build_running(
            &sub_agent_id,
            SubAgentConfig {
                agent_type: AgentTypeFQN::from("fqn_infra_agent"),
            },
        );

        // Create the Super Agent and run Sub Agents
        let super_agent = SuperAgent::new_custom(
            Some(MockStartedOpAMPClientMock::new()),
            &hash_repository_mock,
            sub_agent_builder,
            sub_agents_config_store,
            &sub_agent_hash_repository_mock,
            sub_agent_values_repo,
        );

        assert!(super_agent
            .process_sub_agent_remote_config(remote_config, &mut sub_agents, tx, ctx)
            .is_ok());
    }

    // TODO Move to SubAgent when its event loop is created
    #[test]
    fn receive_sub_agent_remote_deleted_config_should_delete_and_use_local() {
        let ctx = Context::new();
        let (tx, _) = mpsc::channel();

        let hash_repository_mock = MockHashRepositoryMock::new();
        let mut sub_agent_builder = MockSubAgentBuilderMock::new();
        let mut sub_agents_config_store = MockSubAgentsConfigStore::new();
        let mut sub_agent_hash_repository_mock = MockHashRepositoryMock::new();
        let mut sub_agent_values_repo = MockRemoteValuesRepositoryMock::new();

        // Given that we have 3 running Sub Agents
        let mut sub_agents = StartedSubAgents::from(HashMap::from([
            (
                AgentID::new("fluent_bit").unwrap(),
                MockStartedSubAgent::new(),
            ),
            (
                AgentID::new("infra_agent").unwrap(),
                MockStartedSubAgent::new(),
            ),
            (AgentID::new("nrdot").unwrap(), MockStartedSubAgent::new()),
        ]));

        let sub_agent_id = AgentID::new("infra_agent").unwrap();

        // When we receive an empty remote config for a Sub Agent
        let remote_config = RemoteConfig {
            agent_id: sub_agent_id.clone(),
            hash: Hash::new("sub-agent-hash".to_string()),
            config_map: ConfigMap::new(HashMap::from([("".to_string(), "".to_string())])),
        };

        // Then hash repository should save the received hash
        sub_agent_hash_repository_mock
            .should_save_hash(&remote_config.agent_id, &remote_config.hash);
        // And config should be deleted
        sub_agent_values_repo.should_delete_remote(&sub_agent_id);
        // And we reload the config from the Sub Agent Config Store
        let sub_agents_config = SubAgentsConfig::from(HashMap::from([
            (
                AgentID::new("nrdot").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::from("fqn_rdot"),
                },
            ),
            (
                AgentID::new("infra_agent").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::from("fqn_infra_agent"),
                },
            ),
            (
                AgentID::new("fluent_bit").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::from("fqn_fluent_bit"),
                },
            ),
        ]));
        sub_agents_config_store.should_load(&sub_agents_config);
        // And the Sub Agent should be stopped
        sub_agents.get(&sub_agent_id).should_stop();
        // And the Sub Agent should be re-created
        sub_agent_builder.should_build_running(
            &sub_agent_id,
            SubAgentConfig {
                agent_type: AgentTypeFQN::from("fqn_infra_agent"),
            },
        );

        // Create the Super Agent and rub Sub Agents
        let super_agent = SuperAgent::new_custom(
            Some(MockStartedOpAMPClientMock::new()),
            &hash_repository_mock,
            sub_agent_builder,
            sub_agents_config_store,
            &sub_agent_hash_repository_mock,
            sub_agent_values_repo,
        );

        assert!(super_agent
            .process_sub_agent_remote_config(remote_config, &mut sub_agents, tx, ctx)
            .is_ok());
    }

    #[test]
    fn restart_sub_agent() {
        let sub_agent_hash_repository_mock = MockHashRepositoryMock::new();
        let mut sub_agent_builder = MockSubAgentBuilderMock::new();

        // Super Agent OpAMP
        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_health(1);
        started_client.should_stop(1);

        //Sub Agent reload expectations
        let agent_id_to_restart = AgentID::new("infra_agent").unwrap();

        let mut sub_agents_config_store = MockSubAgentsConfigStore::new();
        sub_agents_config_store
            .expect_load()
            .returning(|| Ok(sub_agents_default_config()));

        let mut hash_repository_mock = MockHashRepositoryMock::new();
        hash_repository_mock.expect_get().times(1).returning(|_| {
            let mut hash = Hash::new("a-hash".to_string());
            hash.apply();
            Ok(hash)
        });

        // it should build three subagents (2 + 1 recreation)
        sub_agent_builder.should_build(3);

        let sub_agent_values_repo = MockRemoteValuesRepositoryMock::new();

        // two agents in the supervisor group
        let agent = SuperAgent::new_custom(
            Some(started_client),
            &hash_repository_mock,
            sub_agent_builder,
            sub_agents_config_store,
            &sub_agent_hash_repository_mock,
            sub_agent_values_repo,
        );

        let ctx = Context::new();
        // restart agent after 50 milliseconds
        send_event_after(
            ctx.clone(),
            SubAgentEvent::ConfigUpdated(agent_id_to_restart.clone()).into(),
            Duration::from_millis(50),
        );
        // stop all agents after 100 milliseconds
        send_event_after(
            ctx.clone(),
            SuperAgentEvent::StopRequested.into(),
            Duration::from_millis(300),
        );
        assert!(agent.run(ctx).is_ok())
    }

    #[test]
    fn reload_sub_agent_config_error_on_assemble_new_config() {
        let mut sub_agent_builder = MockSubAgentBuilderMock::new();
        let sub_agent_hash_repository_mock = MockHashRepositoryMock::new();
        let sub_agent_values_repo = MockRemoteValuesRepositoryMock::new();
        let mut sub_agents_config_store = MockSubAgentsConfigStore::new();
        let mut hash_repository_mock = MockHashRepositoryMock::new();

        // Sub Agents
        let sub_agents_config = sub_agents_config_single_agent();

        // it should build one subagent: infra_agent and be called a second time sending the error to opamp
        sub_agent_builder.should_build(1);
        sub_agent_builder.should_not_build(1);

        let agent_id_to_restart = AgentID::new("infra_agent").unwrap();
        //Persister will fail loading new configuration

        sub_agents_config_store
            .expect_load()
            .times(2)
            .returning(move || Ok(sub_agents_config.clone()));

        hash_repository_mock.should_get_hash(
            &AgentID::new_super_agent_id(),
            Hash::applied("a-hash".to_string()),
        );

        // two agents in the supervisor group
        let agent = SuperAgent::new_custom(
            Some(MockStartedOpAMPClientMock::new()),
            &hash_repository_mock,
            sub_agent_builder,
            sub_agents_config_store,
            &sub_agent_hash_repository_mock,
            sub_agent_values_repo,
        );

        let ctx = Context::new();
        // restart agent after 50 milliseconds
        send_event_after(
            ctx.clone(),
            SubAgentEvent::ConfigUpdated(agent_id_to_restart.clone()).into(),
            Duration::from_millis(50),
        );
        // stop all agents after 100 milliseconds
        send_event_after(
            ctx.clone(),
            SuperAgentEvent::StopRequested.into(),
            Duration::from_millis(300),
        );

        let result = agent.run(ctx);

        assert_eq!(
            "``error creating Sub Agent: `error creating sub agent```".to_string(),
            result.err().unwrap().to_string()
        );
    }

    #[test]
    fn recreate_agent_no_errors() {
        let sub_agent_hash_repository_mock = MockHashRepositoryMock::new();
        let sub_agent_values_repo = MockRemoteValuesRepositoryMock::new();

        // Super Agent OpAMP
        let mut started_client = MockStartedOpAMPClientMock::new();
        started_client.should_set_health(1);
        started_client.should_stop(1);

        // recreate agent
        //Sub Agent reload expectations
        let agent_id_to_restart = AgentID::new("infra_agent").unwrap();

        let mut sub_agent_builder = MockSubAgentBuilderMock::new();
        // it should build three sub_agents (2 + 1)
        sub_agent_builder.should_build(3);

        let mut hash_repository_mock = MockHashRepositoryMock::new();
        hash_repository_mock.should_get_hash(
            &AgentID::new_super_agent_id(),
            Hash::applied("a-hash".to_string()),
        );

        let mut sub_agents_config_store = MockSubAgentsConfigStore::new();
        sub_agents_config_store
            .expect_load()
            .returning(|| Ok(sub_agents_default_config()));

        let mut sub_agents_config_store = MockSubAgentsConfigStore::new();
        sub_agents_config_store
            .expect_load()
            .returning(|| Ok(sub_agents_default_config()));

        // Create the Super Agent and rub Sub Agents
        let super_agent = SuperAgent::new_custom(
            Some(started_client),
            &hash_repository_mock,
            sub_agent_builder,
            sub_agents_config_store,
            &sub_agent_hash_repository_mock,
            sub_agent_values_repo,
        );

        let ctx = Context::new();
        // restart agent after 50 milliseconds
        send_event_after(
            ctx.clone(),
            SubAgentEvent::ConfigUpdated(agent_id_to_restart.clone()).into(),
            Duration::from_millis(50),
        );
        // stop all agents after 100 milliseconds
        send_event_after(
            ctx.clone(),
            SuperAgentEvent::StopRequested.into(),
            Duration::from_millis(100),
        );

        assert!(super_agent.run(ctx).is_ok());
    }

    #[test]
    fn create_stop_sub_agents_from_remote_config() {
        let ctx = Context::new();
        // Mocked services
        let sub_agent_hash_repository_mock = MockHashRepositoryMock::new();
        let sub_agent_values_repo = MockRemoteValuesRepositoryMock::new();

        // Sub Agents
        let sub_agents_config = sub_agents_default_config();

        let mut sub_agent_builder = MockSubAgentBuilderMock::new();
        // it should build three times (2 + 1 + 1)
        sub_agent_builder.should_build(3);

        let mut sub_agents_config_store = MockSubAgentsConfigStore::new();
        // all agents on first load
        sub_agents_config_store
            .expect_load()
            .times(1)
            .returning(|| Ok(sub_agents_default_config()));

        sub_agents_config_store
            .expect_load()
            .once()
            .return_once(|| {
                Ok(HashMap::from([(
                    AgentID::new("nrdot").unwrap(),
                    SubAgentConfig {
                        agent_type: AgentTypeFQN::from("newrelic/io.opentelemetry.collector:0.0.1"),
                    },
                )])
                .into())
            });

        sub_agents_config_store
            .expect_store()
            .times(1)
            .returning(|_| Ok(()));

        sub_agents_config_store
            .expect_store()
            .times(1)
            .returning(|_| Ok(()));

        let mut hash_repository_mock = MockHashRepositoryMock::new();
        hash_repository_mock.should_save_hash(
            &AgentID::new_super_agent_id(),
            &Hash::new("a-hash".to_string()),
        );
        hash_repository_mock.should_save_hash(
            &AgentID::new_super_agent_id(),
            &Hash::new("b-hash".to_string()),
        );

        // Create the Super Agent and rub Sub Agents
        let super_agent = SuperAgent::new_custom(
            None::<MockStartedOpAMPClientMock<SuperAgentCallbacks>>,
            &hash_repository_mock,
            sub_agent_builder,
            sub_agents_config_store,
            &sub_agent_hash_repository_mock,
            sub_agent_values_repo,
        );

        let (tx, _) = mpsc::channel();

        let sub_agents = super_agent.load_sub_agents(&sub_agents_config, &tx, ctx.clone());

        let mut running_sub_agents = sub_agents.unwrap().run().unwrap();

        // just one agent, it should remove the infra_agent
        let remote_config = RemoteConfig {
            agent_id: AgentID::new_super_agent_id(),
            hash: Hash::new("a-hash".to_string()),
            config_map: ConfigMap::new(HashMap::from([(
                "".to_string(),
                r#"
agents:
  nrdot:
    agent_type: newrelic/io.opentelemetry.collector:0.0.1
"#
                .to_string(),
            )])),
        };

        assert_eq!(running_sub_agents.len(), 2);

        super_agent
            .apply_remote_config(
                remote_config,
                tx.clone(),
                &mut running_sub_agents,
                ctx.clone(),
            )
            .unwrap();

        assert_eq!(running_sub_agents.len(), 1);

        // remove nrdot and create new infra_agent sub_agent
        let remote_config = RemoteConfig {
            agent_id: AgentID::new_super_agent_id(),
            hash: Hash::new("b-hash".to_string()),
            config_map: ConfigMap::new(HashMap::from([(
                "".to_string(),
                r#"
agents:
  infra_agent:
    agent_type: newrelic/com.newrelic.infrastructure_agent:0.0.1
"#
                .to_string(),
            )])),
        };

        super_agent
            .apply_remote_config(remote_config, tx, &mut running_sub_agents, ctx.clone())
            .unwrap();

        assert_eq!(running_sub_agents.len(), 1);

        assert!(running_sub_agents.stop().is_ok())
    }

    // Invalid configuration should be reported to OpAMP as Failed and the Super Agent should
    // not apply it nor crash execution.
    #[test]
    fn super_agent_invalid_remote_config_should_be_reported_as_failed() {
        let ctx = Context::new();
        let (tx, _) = mpsc::channel();
        // Mocked services
        let sub_agent_hash_repository_mock = MockHashRepositoryMock::new();
        let sub_agent_values_repo = MockRemoteValuesRepositoryMock::new();
        let sub_agent_builder = MockSubAgentBuilderMock::new();
        let mut sub_agents_config_store = MockSubAgentsConfigStore::new();
        let hash_repository_mock = MockHashRepositoryMock::new();
        let mut started_client = MockStartedOpAMPClientMock::new();

        // Structs
        let mut running_sub_agents = StartedSubAgents::default();
        let old_sub_agents_config = SubAgentsConfig::default();
        let agent_id = AgentID::new_super_agent_id();
        let mut remote_config = RemoteConfig {
            agent_id,
            hash: Hash::new("this-is-a-hash".to_string()),
            config_map: ConfigMap::new(HashMap::from([(
                "".to_string(),
                "invalid_yaml_content:{}".to_string(),
            )])),
        };

        //Expectations

        // Report config status as applying
        let status = RemoteConfigStatus {
            status: Applying as i32,
            last_remote_config_hash: remote_config.hash.get().into_bytes(),
            error_message: "".to_string(),
        };
        started_client.should_set_remote_config_status(status);

        // load current sub agents config
        sub_agents_config_store.should_load(&old_sub_agents_config);

        // report failed after trying to unserialize
        let status = RemoteConfigStatus {
            status: Failed as i32,
            last_remote_config_hash: remote_config.hash.get().into_bytes(),
            error_message: "Error applying Super Agent remote config: could not resolve config: `configuration is not valid YAML: `invalid type: string \"invalid_yaml_content:{}\", expected struct SubAgentsConfig``".to_string(),
        };
        started_client.should_set_remote_config_status(status);

        // Create the Super Agent and rub Sub Agents
        let super_agent = SuperAgent::new_custom(
            None,
            &hash_repository_mock,
            sub_agent_builder,
            sub_agents_config_store,
            &sub_agent_hash_repository_mock,
            sub_agent_values_repo,
        );

        super_agent
            .process_super_agent_remote_config(
                &started_client,
                &mut remote_config,
                tx,
                &mut running_sub_agents,
                ctx,
            )
            .unwrap();
    }

    #[test]
    fn super_agent_valid_remote_config_should_be_reported_as_applied() {
        let ctx = Context::new();
        let (tx, _) = mpsc::channel();
        // Mocked services
        let sub_agent_hash_repository_mock = MockHashRepositoryMock::new();
        let sub_agent_values_repo = MockRemoteValuesRepositoryMock::new();
        let sub_agent_builder = MockSubAgentBuilderMock::new();
        let mut sub_agents_config_store = MockSubAgentsConfigStore::new();
        let mut hash_repository_mock = MockHashRepositoryMock::new();
        let mut started_client = MockStartedOpAMPClientMock::new();

        // Structs
        let mut started_sub_agent = MockStartedSubAgent::new();
        let sub_agent_id = AgentID::try_from("agent_id".to_string()).unwrap();
        started_sub_agent.should_stop();

        let mut running_sub_agents =
            StartedSubAgents::from(HashMap::from([(sub_agent_id.clone(), started_sub_agent)]));

        let old_sub_agents_config = SubAgentsConfig::from(HashMap::from([(
            sub_agent_id.clone(),
            SubAgentConfig {
                agent_type: "some_agent_type".into(),
            },
        )]));

        let agent_id = AgentID::new_super_agent_id();
        let mut remote_config = RemoteConfig {
            agent_id,
            hash: Hash::new("this-is-a-hash".to_string()),
            config_map: ConfigMap::new(HashMap::from([("".to_string(), "agents: {}".to_string())])),
        };

        //Expectations

        // Report config status as applying
        let status = RemoteConfigStatus {
            status: Applying as i32,
            last_remote_config_hash: remote_config.hash.get().into_bytes(),
            error_message: "".to_string(),
        };
        started_client.should_set_remote_config_status(status);

        // load current sub agents config
        sub_agents_config_store.should_load(&old_sub_agents_config);

        // store remote config with empty agents
        sub_agents_config_store.should_store(&SubAgentsConfig::default());

        // persist hash
        hash_repository_mock.should_save_hash(&AgentID::new_super_agent_id(), &remote_config.hash);

        // persist hash after applied
        let mut applied_hash = remote_config.hash.clone();
        applied_hash.apply();
        hash_repository_mock.should_save_hash(&AgentID::new_super_agent_id(), &applied_hash);

        // Report config status as applied
        let status = RemoteConfigStatus {
            status: Applied as i32,
            last_remote_config_hash: remote_config.hash.get().into_bytes(),
            error_message: "".to_string(),
        };
        started_client.should_set_remote_config_status(status);

        // Create the Super Agent and rub Sub Agents
        let super_agent = SuperAgent::new_custom(
            None,
            &hash_repository_mock,
            sub_agent_builder,
            sub_agents_config_store,
            &sub_agent_hash_repository_mock,
            sub_agent_values_repo,
        );

        super_agent
            .process_super_agent_remote_config(
                &started_client,
                &mut remote_config,
                tx,
                &mut running_sub_agents,
                ctx,
            )
            .unwrap();
    }

    ////////////////////////////////////////////////////////////////////////////////////
    // Test helpers
    ////////////////////////////////////////////////////////////////////////////////////

    fn sub_agents_config_single_agent() -> SubAgentsConfig {
        HashMap::from([(
            AgentID::new("infra_agent").unwrap(),
            SubAgentConfig {
                agent_type: AgentTypeFQN::from("newrelic/com.newrelic.infrastructure_agent:0.0.1"),
            },
        )])
        .into()
    }

    fn sub_agents_default_config() -> SubAgentsConfig {
        HashMap::from([
            (
                AgentID::new("infra_agent").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::from(
                        "newrelic/com.newrelic.infrastructure_agent:0.0.1",
                    ),
                },
            ),
            (
                AgentID::new("nrdot").unwrap(),
                SubAgentConfig {
                    agent_type: AgentTypeFQN::from("newrelic/io.opentelemetry.collector:0.0.1"),
                },
            ),
        ])
        .into()
    }

    fn send_event_after(ctx: Context<Option<Event>>, event: Event, after: Duration) {
        spawn({
            let ctx = ctx.clone();
            move || {
                sleep(after);
                ctx.cancel_all(Some(event)).unwrap();
            }
        });
    }
}
