use crate::opamp::remote_config_hash::Hash;
use futures::executor::block_on;
use opamp_client::error::ClientError;
use opamp_client::opamp::proto::{RemoteConfigStatus, RemoteConfigStatuses};
use opamp_client::StartedClient;

use super::SubAgentCallbacks;

pub mod client_builder;
pub mod common;
pub mod remote_config_publisher;

pub fn report_remote_config_status_applying<O>(
    opamp_client: &O,
    hash: &Hash,
) -> Result<(), ClientError>
where
    O: StartedClient<SubAgentCallbacks>,
{
    let err = "".to_string();
    report_remote_config_status(
        opamp_client,
        hash,
        RemoteConfigStatuses::Applying as i32,
        err,
    )
}

pub fn report_remote_config_status_error<O>(
    opamp_client: &O,
    hash: &Hash,
    error_msg: String,
) -> Result<(), ClientError>
where
    O: StartedClient<SubAgentCallbacks>,
{
    report_remote_config_status(
        opamp_client,
        hash,
        RemoteConfigStatuses::Failed as i32,
        error_msg,
    )
}

pub fn report_remote_config_status_applied<O>(
    opamp_client: &O,
    hash: &Hash,
) -> Result<(), ClientError>
where
    O: StartedClient<SubAgentCallbacks>,
{
    let err = "".to_string();
    report_remote_config_status(
        opamp_client,
        hash,
        RemoteConfigStatuses::Applied as i32,
        err,
    )
}

fn report_remote_config_status<O>(
    opamp_client: &O,
    hash: &Hash,
    status: i32,
    error_msg: String,
) -> Result<(), ClientError>
where
    O: StartedClient<SubAgentCallbacks>,
{
    block_on(opamp_client.set_remote_config_status(RemoteConfigStatus {
        last_remote_config_hash: hash.get().into_bytes(),
        status,
        error_message: error_msg,
    }))
}
