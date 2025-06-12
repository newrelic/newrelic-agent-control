use crate::opamp::remote_config::Hash;
use opamp_client::opamp::proto::RemoteConfigStatus;
use opamp_client::opamp::proto::RemoteConfigStatuses;
use opamp_client::{ClientError, StartedClient};

use crate::opamp::remote_config::hash::ConfigState;

pub fn report_state<C: StartedClient>(
    state: ConfigState,
    hash: Hash,
    opamp_client: &C,
) -> Result<(), ClientError> {
    opamp_client.set_remote_config_status(RemoteConfigStatus {
        last_remote_config_hash: hash.to_string().into_bytes(),
        status: RemoteConfigStatuses::from(state.clone()) as i32,
        error_message: state.error_message().cloned().unwrap_or_default(),
    })
}

impl From<ConfigState> for RemoteConfigStatuses {
    fn from(value: ConfigState) -> Self {
        match value {
            ConfigState::Applying => RemoteConfigStatuses::Applying,
            ConfigState::Applied => RemoteConfigStatuses::Applied,
            ConfigState::Failed { .. } => RemoteConfigStatuses::Failed,
        }
    }
}
