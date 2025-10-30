use std::{marker::PhantomData, sync::Arc};

use serde::{Serialize, de::DeserializeOwned};
use tracing::debug;

use crate::{
    agent_control::{agent_id::AgentID, defaults::STORE_KEY_INSTANCE_ID},
    opamp::{
        data_store::{OpAMPDataStore, OpAMPDataStoreError},
        instance_id::getter::DataStored,
    },
};

use super::{definition::InstanceIdentifiers, getter::GetterError};

pub struct Storer<D, I>
where
    D: OpAMPDataStore,
    I: InstanceIdentifiers + Serialize + DeserializeOwned,
{
    opamp_data_store: Arc<D>,
    _identifiers: PhantomData<I>,
}

impl<D, I> From<Arc<D>> for Storer<D, I>
where
    D: OpAMPDataStore,
    I: InstanceIdentifiers + Serialize + DeserializeOwned,
{
    fn from(opamp_data_store: Arc<D>) -> Self {
        Self {
            opamp_data_store,
            _identifiers: PhantomData,
        }
    }
}

impl<D, I> InstanceIDStorer for Storer<D, I>
where
    D: OpAMPDataStore,
    I: InstanceIdentifiers + Serialize + DeserializeOwned,
{
    type Error = OpAMPDataStoreError;

    type Identifiers = I;

    fn set(&self, agent_id: &AgentID, data: &DataStored<I>) -> Result<(), OpAMPDataStoreError> {
        debug!("storer: setting Instance ID of agent_id: {}", agent_id);

        self.opamp_data_store
            .set_opamp_data(agent_id, STORE_KEY_INSTANCE_ID, data)?;

        Ok(())
    }

    fn get(&self, agent_id: &AgentID) -> Result<Option<DataStored<I>>, OpAMPDataStoreError> {
        debug!("storer: getting Instance ID of agent_id: {}", agent_id);

        if let Some(data) = self
            .opamp_data_store
            .get_opamp_data(agent_id, STORE_KEY_INSTANCE_ID)?
        {
            return Ok(Some(data));
        }

        Ok(None)
    }
}

pub trait InstanceIDStorer
where
    GetterError: From<Self::Error>,
{
    type Error;
    type Identifiers: InstanceIdentifiers;

    fn set(
        &self,
        agent_id: &AgentID,
        data: &DataStored<Self::Identifiers>,
    ) -> Result<(), Self::Error>;
    fn get(&self, agent_id: &AgentID)
    -> Result<Option<DataStored<Self::Identifiers>>, Self::Error>;
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::opamp::instance_id::definition::tests::MockIdentifiers;

    use super::*;
    use mockall::mock;
    use thiserror::Error;

    #[derive(Error, Debug, PartialEq, Clone)]
    #[error("mock getter error")]
    pub struct MockStorerError;

    impl From<MockStorerError> for GetterError {
        fn from(_: MockStorerError) -> Self {
            GetterError::MockGetterError
        }
    }

    mock! {
        pub InstanceIDStorer {}

        impl InstanceIDStorer for InstanceIDStorer {
            type Error = MockStorerError;
            type Identifiers = MockIdentifiers;

            fn set(&self, agent_id: &AgentID, data: &DataStored<MockIdentifiers>) -> Result<(), MockStorerError>;
            fn get(&self, agent_id: &AgentID) -> Result<Option<DataStored<MockIdentifiers>>, MockStorerError>;
        }
    }
}
