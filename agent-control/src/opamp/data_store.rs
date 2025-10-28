use std::io;

use serde::{Serialize, de::DeserializeOwned};
use thiserror::Error;

use crate::{agent_control::agent_id::AgentID, k8s};

#[derive(Debug, Error)]
pub enum OpAMPDataStoreError {
    #[error("host I/O error: {0}")]
    Io(io::Error),
    #[error("k8s error: {0}")]
    K8s(k8s::Error),
}

/// The key used to identify the data in the OpAMP Data Store.
pub type StoreKey = str;

/// Implementers of this trait represent data stores for OpAMP-related data.
///
/// They expose ways to get, set and delete data associated with the management of agent
/// workloads in a way that matches the OpAMP specification.
///
/// The data to be written/read needs to be serializable/deserializable via Serde.
pub trait OpAMPDataStore {
    fn get_opamp_data<T>(
        &self,
        agent_id: &AgentID,
        key: &str,
    ) -> Result<Option<T>, OpAMPDataStoreError>
    where
        T: DeserializeOwned;

    fn get_local_data<T>(
        &self,
        agent_id: &AgentID,
        key: &str,
    ) -> Result<Option<T>, OpAMPDataStoreError>
    where
        T: DeserializeOwned;

    fn set_opamp_data<T>(
        &self,
        agent_id: &AgentID,
        key: &str,
        data: &T,
    ) -> Result<(), OpAMPDataStoreError>
    where
        T: Serialize;

    /// Delete data in the specified StoreKey of an Agent store.
    fn delete_opamp_data(&self, agent_id: &AgentID, key: &str) -> Result<(), OpAMPDataStoreError>;
}
