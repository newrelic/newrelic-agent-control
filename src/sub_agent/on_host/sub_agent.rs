use std::thread::JoinHandle;

use futures::executor::block_on;
use opamp_client::opamp::proto::AgentHealth;
use opamp_client::{Client, StartedClient};
use tracing::{error, info};

use super::supervisor::command_supervisor::{NotStartedSupervisorOnHost, StartedSupervisorOnHost};
use crate::config::remote_config_hash::{Hash, HashRepository, HashRepositoryFile};
use crate::config::super_agent_configs::{AgentID, AgentTypeFQN};
use crate::context::Context;
use crate::opamp::client_builder::OpAMPClientBuilder;
use crate::sub_agent::error::SubAgentError;
use crate::sub_agent::on_host::opamp::build_opamp_and_start_client;
use crate::sub_agent::{NotStartedSubAgent, StartedSubAgent};
use crate::super_agent::instance_id::InstanceIDGetter;
use crate::utils::time::get_sys_time_nano;
use opamp_client::opamp::proto::{RemoteConfigStatus, RemoteConfigStatuses};

////////////////////////////////////////////////////////////////////////////////////
// Not Started SubAgent On Host
// C: OpAMP Client
////////////////////////////////////////////////////////////////////////////////////
pub struct NotStartedSubAgentOnHost<B, HR = HashRepositoryFile>
where
    B: OpAMPClientBuilder,
    HR: HashRepository,
{
    opamp_client: Option<B::Client>,
    supervisors: Vec<NotStartedSupervisorOnHost>,
    agent_id: AgentID,
    remote_config_hash_repository: HR,
}

impl<B, HR> NotStartedSubAgentOnHost<B, HR>
where
    B: OpAMPClientBuilder,
    HR: HashRepository,
{
    pub fn new<'a, ID: InstanceIDGetter>(
        agent_id: AgentID,
        supervisors: Vec<NotStartedSupervisorOnHost>,
        opamp_builder: Option<&'a B>,
        instance_id_getter: &'a ID,
        agent_type: AgentTypeFQN,
        remote_config_hash_repository: HR,
    ) -> Result<Self, SubAgentError> {
        let opamp_client = build_opamp_and_start_client(
            Context::new(),
            opamp_builder,
            instance_id_getter,
            agent_id.clone(),
            &agent_type,
        )?;

        Ok(NotStartedSubAgentOnHost {
            opamp_client,
            supervisors,
            agent_id,
            remote_config_hash_repository,
        })
    }

    pub fn agent_id(&self) -> &AgentID {
        &self.agent_id
    }

    fn set_config_hash_as_applied(&self, mut hash: Hash) -> Result<(), SubAgentError> {
        if let Some(opamp_handle) = &self.opamp_client {
            block_on(opamp_handle.set_remote_config_status(RemoteConfigStatus {
                last_remote_config_hash: hash.get().into_bytes(),
                status: RemoteConfigStatuses::Applied as i32,
                ..Default::default()
            }))?;
            hash.apply();
            self.remote_config_hash_repository
                .save(&self.agent_id, &hash)?;
        }

        Ok(())
    }
}

impl<B, HR> NotStartedSubAgent for NotStartedSubAgentOnHost<B, HR>
where
    B: OpAMPClientBuilder,
    HR: HashRepository,
{
    type RunningSubAgent = StartedSubAgentOnHost<B, HR>;

    fn run(self) -> Result<Self::RunningSubAgent, SubAgentError> {
        let agent_id = self.agent_id.clone();
        if self.opamp_client.is_some() {
            // TODO should we error on first launch with no hash file?
            let remote_config_hash = self
                .remote_config_hash_repository
                .get(&AgentID(agent_id.to_string()))
                .map_err(|e| error!("hash repository error: {}", e))
                .ok();

            if let Some(hash) = remote_config_hash {
                if !hash.is_applied() {
                    self.set_config_hash_as_applied(hash)?;
                }
            }
        }

        let started_supervisors = self
            .supervisors
            .into_iter()
            .map(|s| s.run())
            .collect::<Result<Vec<_>, _>>()?;

        let started_sub_agent = StartedSubAgentOnHost {
            opamp_client: self.opamp_client,
            supervisors: started_supervisors,
            agent_id: self.agent_id,
            remote_config_hash_repository: self.remote_config_hash_repository,
        };

        Ok(started_sub_agent)
    }
}

pub struct StartedSubAgentOnHost<B, HR = HashRepositoryFile>
where
    B: OpAMPClientBuilder,
    HR: HashRepository,
{
    opamp_client: Option<B::Client>,
    supervisors: Vec<StartedSupervisorOnHost>,
    agent_id: AgentID,
    remote_config_hash_repository: HR,
}

impl<B, HR> StartedSubAgent for StartedSubAgentOnHost<B, HR>
where
    B: OpAMPClientBuilder,
    HR: HashRepository,
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

        let stopped_supervisors = self.supervisors.into_iter().map(|s| s.stop()).collect();

        Ok(stopped_supervisors)
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Started SubAgent On Host
// C: OpAMP Client
////////////////////////////////////////////////////////////////////////////////////
/*pub struct StartedSubAgentOnHost<C>
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
}*/
