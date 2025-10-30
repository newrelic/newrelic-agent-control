use std::{io, marker::PhantomData, sync::Arc};

use serde::{Serialize, de::DeserializeOwned};
use thiserror::Error;
use tracing::debug;

use crate::{
    agent_control::{agent_id::AgentID, defaults::STORE_KEY_INSTANCE_ID},
    k8s,
    opamp::{data_store::OpAMPDataStore, instance_id::getter::DataStored},
};

use super::definition::InstanceIdentifiers;

pub struct Storer<D, I>
where
    D: OpAMPDataStore,
    I: InstanceIdentifiers + Serialize + DeserializeOwned,
{
    opamp_data_store: Arc<D>,
    // The `PhantomData` for `I` is used because the `InstanceIDStorer` implementation needs to
    // provide an implementer of `InstanceIdentifiers` as its `Identifiers` associated type, but
    // the struct cannot be parameterized over `I` if it does not use it (errors with
    // "Parameter `I` is never used").
    // To be able to refer to `I` in the impl block, we include this `PhantomData` so
    // there's an "usage" of `I` in the structure. Otherwise we cannot
    // parameterize over `I` in this struct (errors with "Parameter `I` is never used").
    // We might be able to remove this once we remove the `InstanceIDStorer` and `InstanceIDGetter`
    // traits.
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

#[derive(Debug, Error)]
pub enum StorerError {
    #[error("host I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("k8s error: {0}")]
    K8s(#[from] k8s::Error),
}

impl<D, I> InstanceIDStorer for Storer<D, I>
where
    D: OpAMPDataStore,
    I: InstanceIdentifiers + Serialize + DeserializeOwned,
    StorerError: From<D::Error>,
{
    type Identifiers = I;

    fn set(&self, agent_id: &AgentID, data: &DataStored<I>) -> Result<(), StorerError> {
        debug!("storer: setting Instance ID of agent_id: {}", agent_id);

        self.opamp_data_store
            .set_opamp_data(agent_id, STORE_KEY_INSTANCE_ID, data)?;

        Ok(())
    }

    fn get(&self, agent_id: &AgentID) -> Result<Option<DataStored<I>>, StorerError> {
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

pub trait InstanceIDStorer {
    type Identifiers: InstanceIdentifiers;

    fn set(
        &self,
        agent_id: &AgentID,
        data: &DataStored<Self::Identifiers>,
    ) -> Result<(), StorerError>;
    fn get(&self, agent_id: &AgentID)
    -> Result<Option<DataStored<Self::Identifiers>>, StorerError>;
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::opamp::instance_id::definition::tests::MockIdentifiers;

    use super::*;
    use mockall::mock;

    mock! {
        pub InstanceIDStorer {}

        impl InstanceIDStorer for InstanceIDStorer {
            type Identifiers = MockIdentifiers;

            fn set(&self, agent_id: &AgentID, data: &DataStored<MockIdentifiers>) -> Result<(), StorerError>;
            fn get(&self, agent_id: &AgentID) -> Result<Option<DataStored<MockIdentifiers>>, StorerError>;
        }
    }
}
