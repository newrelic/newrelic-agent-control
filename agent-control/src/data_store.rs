use std::error::Error;

use serde::{Serialize, de::DeserializeOwned};

use crate::{agent_control::agent_id::AgentID, opamp::instance_id::storer::StorerError};

/// The key used to identify the data in the OpAMP Data Store.
pub type StoreKey = str;

/// Implementations of this trait represent capability to perform data R/W on some platform.
///
/// Examples would be filesystem I/O for hosts or the API server for Kubernetes.
///
/// The methods provide ways to get (local or remote), set and delete remote data associated with
/// the management of agent workloads.
///
/// The data to be written/read needs to be serializable/deserializable via Serde.
pub trait DataStore {
    type Error: Error + Into<StorerError>;

    fn get_remote_data<T>(&self, agent_id: &AgentID, key: &str) -> Result<Option<T>, Self::Error>
    where
        T: DeserializeOwned;

    fn get_local_data<T>(&self, agent_id: &AgentID, key: &str) -> Result<Option<T>, Self::Error>
    where
        T: DeserializeOwned;

    fn set_remote_data<T>(
        &self,
        agent_id: &AgentID,
        key: &str,
        data: &T,
    ) -> Result<(), Self::Error>
    where
        T: Serialize;

    fn delete_remote_data(&self, agent_id: &AgentID, key: &str) -> Result<(), Self::Error>;
}
