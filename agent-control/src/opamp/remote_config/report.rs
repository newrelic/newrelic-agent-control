use super::hash::ConfigState;
use super::hash::Hash;
use opamp_client::opamp::proto::RemoteConfigStatus;
use opamp_client::opamp::proto::RemoteConfigStatuses;
use opamp_client::{ClientError, StartedClient};

#[derive(Debug, Clone)]
pub enum OpampRemoteConfigStatus {
    Applying,
    Error(String),
    Applied,
}

impl OpampRemoteConfigStatus {
    fn as_remote_config_status_i32(&self) -> i32 {
        match self {
            Self::Applying => RemoteConfigStatuses::Applying as i32,
            Self::Error(_) => RemoteConfigStatuses::Failed as i32,
            Self::Applied => RemoteConfigStatuses::Applied as i32,
        }
    }

    fn err_message(self) -> Option<String> {
        match self {
            Self::Error(msg) => Some(msg),
            Self::Applying | Self::Applied => None,
        }
    }

    pub fn report<C>(self, opamp_client: &C, hash: &Hash) -> Result<(), ClientError>
    where
        C: StartedClient,
    {
        opamp_client.set_remote_config_status(RemoteConfigStatus {
            last_remote_config_hash: hash.get().into_bytes(),
            status: self.as_remote_config_status_i32(),
            error_message: self.err_message().unwrap_or_default(),
        })
    }
}

impl From<&Hash> for OpampRemoteConfigStatus {
    fn from(hash: &Hash) -> Self {
        match &hash.state {
            ConfigState::Applying => Self::Applying,
            ConfigState::Applied => Self::Applied,
            ConfigState::Failed { error_message } => Self::Error(error_message.to_owned()),
        }
    }
}
