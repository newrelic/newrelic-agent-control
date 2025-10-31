use std::error::Error;

use serde::{Deserialize, Serialize};

use crate::{agent_control::agent_id::AgentID, opamp::instance_id::getter::GetterError};

/// The key used to identify the data in the OpAMP Data Store.
pub type StoreKey = str;

/// Implementers of this trait represent data stores for OpAMP-related data.
///
/// They expose ways to get, set and delete data associated with the management of agent
/// workloads in a way that matches the OpAMP specification.
///
/// The data to be written/read needs to be serializable/deserializable via Serde.
pub trait OpAMPDataStore {
    type Error: Error + Into<GetterError>;

    fn get_opamp_data<T>(&self, agent_id: &AgentID, key: &str) -> Result<Option<T>, Self::Error>
    where
        T: for<'de> Deserialize<'de> + 'static;

    fn get_local_data<T>(&self, agent_id: &AgentID, key: &str) -> Result<Option<T>, Self::Error>
    where
        T: for<'de> Deserialize<'de> + 'static;

    fn set_opamp_data<T>(&self, agent_id: &AgentID, key: &str, data: &T) -> Result<(), Self::Error>
    where
        T: Serialize + 'static;

    fn delete_opamp_data(&self, agent_id: &AgentID, key: &str) -> Result<(), Self::Error>;
}

#[cfg(test)]
pub mod tests {
    use super::*;

    use mockall::mock;
    use thiserror::Error;

    #[derive(Debug, Error)]
    #[error("Mock data store error")]
    pub struct MockDataStoreError;

    impl From<MockDataStoreError> for GetterError {
        fn from(_: MockDataStoreError) -> Self {
            GetterError::MockGetterError
        }
    }

    mock! {
        pub OpAMPDataStore {}

        impl OpAMPDataStore for OpAMPDataStore {
            type Error = MockDataStoreError;

            fn get_opamp_data<T>(&self, agent_id: &AgentID, key: &str) -> Result<Option<T>, MockDataStoreError>
            where
                T: for<'de> Deserialize<'de> + 'static;

            fn get_local_data<T>(&self, agent_id: &AgentID, key: &str) -> Result<Option<T>, MockDataStoreError>
            where
                T: for<'de> Deserialize<'de> + 'static;

            fn set_opamp_data<T>(&self, agent_id: &AgentID, key: &str, data: &T) -> Result<(), MockDataStoreError>
            where
                T: Serialize + 'static;

            fn delete_opamp_data(&self, agent_id: &AgentID, key: &str) -> Result<(), MockDataStoreError>;
        }
    }
}
