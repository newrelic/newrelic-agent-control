use futures::executor::block_on;
use opamp_client::opamp::proto::{RemoteConfigStatus, RemoteConfigStatuses};
use opamp_client::StartedClient;

use crate::{
    config::store::SubAgentsConfigStore,
    opamp::{remote_config::RemoteConfigError, remote_config_hash::HashRepository},
    sub_agent::SubAgentBuilder,
    super_agent::{
        error::AgentError,
        super_agent::{SuperAgent, SuperAgentCallbacks},
    },
};

impl<'a, S, O, HR, SL> SuperAgent<'a, S, O, HR, SL>
where
    O: StartedClient<SuperAgentCallbacks>,
    HR: HashRepository,
    S: SubAgentBuilder,
    SL: SubAgentsConfigStore,
{
    pub(crate) fn invalid_remote_config(
        &self,
        remote_config_error: RemoteConfigError,
    ) -> Result<(), AgentError> {
        if let Some(opamp_client) = &self.opamp_client {
            self.process_super_agent_remote_config_error(opamp_client, remote_config_error)
        } else {
            unreachable!("got remote config without OpAMP being enabled")
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
}
