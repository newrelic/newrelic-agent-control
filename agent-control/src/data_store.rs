//! Platform-agnostic persistence of the data Agent Control reads and writes when managing
//! agent workloads (local and remote configuration, instance state).

use std::error::Error;

use serde::{Serialize, de::DeserializeOwned};

use crate::{
    agent_control::agent_id::AgentID, opamp::instance_id::storer::StorerError,
    resource_ownership::ResourceOwnership,
};

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
    /// Error type returned by the store's operations; convertible into a [`StorerError`].
    type Error: Error + Into<StorerError>;

    /// Reads the remote data stored for `agent_id` under `key`, returning `None` if absent.
    fn get_remote_data<T>(&self, agent_id: &AgentID, key: &str) -> Result<Option<T>, Self::Error>
    where
        T: DeserializeOwned;

    /// Reads the local data stored for `agent_id` under `key`, returning `None` if absent.
    fn get_local_data<T>(&self, agent_id: &AgentID, key: &str) -> Result<Option<T>, Self::Error>
    where
        T: DeserializeOwned;

    /// Writes `data` as the remote data for `agent_id` under `key`, recording `ownership`.
    fn set_remote_data<T>(
        &self,
        agent_id: &AgentID,
        ownership: ResourceOwnership,
        key: &str,
        data: &T,
    ) -> Result<(), Self::Error>
    where
        T: Serialize;

    /// Deletes the remote data stored for `agent_id` under `key`.
    fn delete_remote_data(&self, agent_id: &AgentID, key: &str) -> Result<(), Self::Error>;
}
