use std::thread::JoinHandle;

use futures::executor::block_on;
use opamp_client::opamp::proto::AgentHealth;
use opamp_client::{Client, StartedClient};
use tracing::{error, info};

use super::supervisor::command_supervisor::{NotStartedSupervisorOnHost, StartedSupervisorOnHost};
use crate::config::remote_config_hash::{Hash, HashRepository, HashRepositoryFile};
use crate::config::super_agent_configs::{AgentID, AgentTypeFQN};
use crate::context::Context;
use crate::opamp::client_builder::{OpAMPClientBuilder, OpAMPClientBuilderError};
use crate::sub_agent::error::SubAgentError;
use crate::sub_agent::on_host::opamp::build_opamp_and_start_client;
use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent};
use crate::super_agent::instance_id::InstanceIDGetter;
use crate::super_agent::super_agent::SuperAgentEvent;
use crate::utils::time::get_sys_time_nano;
use opamp_client::opamp::proto::{RemoteConfigStatus, RemoteConfigStatuses};

////////////////////////////////////////////////////////////////////////////////////
// Not Started SubAgent On Host
// C: OpAMP Client
////////////////////////////////////////////////////////////////////////////////////
pub struct NotStartedSubAgentOnHost<'a, OpAMPBuilder, ID, HR = HashRepositoryFile>
where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
    HR: HashRepository,
{
    opamp_builder: Option<&'a OpAMPBuilder>,
    instance_id_getter: &'a ID,
    supervisors: Vec<NotStartedSupervisorOnHost>,
    agent_id: AgentID,
    agent_type: AgentTypeFQN,
    remote_config_hash_repository: HR,
}

impl<'a, OpAMPBuilder, ID, HR> NotStartedSubAgentOnHost<'a, OpAMPBuilder, ID, HR>
where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
    HR: HashRepository,
{
    pub fn new(
        agent_id: AgentID,
        supervisors: Vec<NotStartedSupervisorOnHost>,
        opamp_builder: Option<&'a OpAMPBuilder>,
        instance_id_getter: &'a ID,
        agent_type: AgentTypeFQN,
        remote_config_hash_repository: HR,
    ) -> Self {
        NotStartedSubAgentOnHost {
            opamp_builder,
            instance_id_getter,
            supervisors,
            agent_id,
            agent_type,
            remote_config_hash_repository,
        }
    }

    pub fn agent_id(&self) -> &AgentID {
        &self.agent_id
    }

    fn run_opamp_client(
        &self,
        ctx: Context<Option<SuperAgentEvent>>,
    ) -> Result<Option<OpAMPBuilder::Client>, OpAMPClientBuilderError> {
        build_opamp_and_start_client(
            ctx,
            self.opamp_builder,
            self.instance_id_getter,
            self.agent_id.clone(),
            &self.agent_type,
        )
    }

    fn set_config_hash_as_applied(
        &self,
        opamp_client: &OpAMPBuilder::Client,
        mut hash: Hash,
    ) -> Result<(), SubAgentError> {
        block_on(opamp_client.set_remote_config_status(RemoteConfigStatus {
            last_remote_config_hash: hash.get().into_bytes(),
            status: RemoteConfigStatuses::Applied as i32,
            ..Default::default()
        }))?;
        hash.apply();
        self.remote_config_hash_repository
            .save(&self.agent_id, &hash)?;
        Ok(())
    }
}

impl<'a, OpAMPBuilder, ID> NotStartedSubAgent for NotStartedSubAgentOnHost<'a, OpAMPBuilder, ID>
where
    OpAMPBuilder: OpAMPClientBuilder,
    ID: InstanceIDGetter,
{
    type StartedSubAgent = StartedSubAgentOnHost<OpAMPBuilder::Client>;

    fn run(self) -> Result<Self::StartedSubAgent, SubAgentError> {
        let agent_id = self.agent_id.clone();
        let started_opamp_client = self.run_opamp_client(Context::new())?;
        // TODO
        if let Some(handle) = &started_opamp_client {
            // TODO should we error on first launch with no hash file?
            let remote_config_hash = self
                .remote_config_hash_repository
                .get(&AgentID(agent_id.to_string()))
                .map_err(|e| error!("hash repository error: {}", e))
                .ok();

            if let Some(hash) = remote_config_hash {
                if !hash.is_applied() {
                    self.set_config_hash_as_applied(handle, hash)?;
                }
            }
        }
        let mut supervisors = Vec::new();
        for supervisor in self.supervisors {
            supervisors.push(supervisor.run()?);
        }
        Ok(StartedSubAgentOnHost::new(
            agent_id,
            started_opamp_client,
            supervisors,
        ))
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Started SubAgent On Host
// C: OpAMP Client
////////////////////////////////////////////////////////////////////////////////////
pub struct StartedSubAgentOnHost<C>
where
    C: StartedClient,
{
    opamp_client: Option<C>,
    supervisors: Vec<StartedSupervisorOnHost>,
    agent_id: AgentID,
}

impl<C> StartedSubAgentOnHost<C>
where
    C: StartedClient,
{
    pub fn new(
        agent_id: AgentID,
        opamp_client: Option<C>,
        supervisors: Vec<StartedSupervisorOnHost>,
    ) -> Self
    where
        C: StartedClient,
    {
        StartedSubAgentOnHost {
            opamp_client,
            supervisors,
            agent_id,
        }
    }

    pub fn agent_id(&self) -> &AgentID {
        &self.agent_id
    }
}

impl<C> StartedSubAgent for StartedSubAgentOnHost<C>
where
    C: StartedClient,
{
    fn stop(self) -> Result<Vec<JoinHandle<()>>, SubAgentError> {
        let _client = match self.opamp_client {
            Some(client) => {
                info!(
                    "Stopping OpAMP client for supervised agent type: {}",
                    self.agent_id
                );
                // set OpAMP health
                block_on(client.set_health(AgentHealth {
                    healthy: false,
                    start_time_unix_nano: get_sys_time_nano()?,
                    last_error: "".to_string(),
                }))?;

                Some(block_on(client.stop())?)
            }
            None => None,
        };

        let mut stopped_runners = Vec::new();
        for supervisors in self.supervisors {
            stopped_runners.push(supervisors.stop());
        }
        Ok(stopped_runners)
    }
}
