use super::hash::Hash;
use opamp_client::opamp::proto::RemoteConfigStatus;
use opamp_client::opamp::proto::RemoteConfigStatuses;
use opamp_client::{operation::callbacks::Callbacks, ClientError, StartedClient};

pub enum RemoteConfigStatusReport {
    Applying,
    Error(String),
    Applied,
}

impl RemoteConfigStatusReport {
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

    pub fn report<O, C>(self, opamp_client: &O, hash: &Hash) -> Result<(), ClientError>
    where
        C: Callbacks,
        O: StartedClient<C>,
    {
        opamp_client.set_remote_config_status(RemoteConfigStatus {
            last_remote_config_hash: hash.get().into_bytes(),
            status: self.as_remote_config_status_i32(),
            error_message: self.err_message().unwrap_or_default(),
        })
    }
}
