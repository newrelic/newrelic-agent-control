use serde::{Serialize, de::DeserializeOwned};

use crate::agent_control::agent_id::AgentID;

/// The key used to identify the data in the OpAMP Data Store.
pub type StoreKey = str;

/// Implementers of this trait represent data stores for OpAMP-related data.
///
/// They expose ways to get, set and delete data associated with the management of agent
/// workloads in a way that matches the OpAMP specification.
///
/// The data to be written/read needs to be serializable/deserializable via Serde.
pub trait OpAMPDataStore {
    type Error;

    fn get_opamp_data<T>(&self, agent_id: &AgentID, key: &str) -> Result<Option<T>, Self::Error>
    where
        T: DeserializeOwned;

    fn get_local_data<T>(&self, agent_id: &AgentID, key: &str) -> Result<Option<T>, Self::Error>
    where
        T: DeserializeOwned;

    fn set_opamp_data<T>(&self, agent_id: &AgentID, key: &str, data: &T) -> Result<(), Self::Error>
    where
        T: Serialize;

    /// Delete data in the specified StoreKey of an Agent store.
    fn delete_opamp_data(&self, agent_id: &AgentID, key: &str) -> Result<(), Self::Error>;
}
