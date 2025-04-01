use crate::agent_control::agent_id::AgentID;
#[cfg(feature = "k8s")]
use crate::k8s::{
    self,
    store::{K8sStore, STORE_KEY_INSTANCE_ID},
};
use crate::opamp::instance_id::getter::DataStored;
use crate::opamp::instance_id::storer::InstanceIDStorer;
use std::sync::Arc;
use tracing::debug;

use super::getter::Identifiers;

pub struct Storer {
    k8s_store: Arc<K8sStore>,
}

#[derive(thiserror::Error, Debug)]
pub enum StorerError {
    #[error("failed to persist on k8s {0}")]
    FailedToPersistK8s(#[from] k8s::Error),

    #[error("generic storer error")]
    Generic,
}

impl InstanceIDStorer for Storer {
    type Error = StorerError;
    type Identifiers = Identifiers;

    fn set(
        &self,
        agent_id: &AgentID,
        ds: &DataStored<Self::Identifiers>,
    ) -> Result<(), Self::Error> {
        debug!("storer: setting Instance ID of agent_id: {}", agent_id);

        self.k8s_store
            .set_opamp_data(agent_id, STORE_KEY_INSTANCE_ID, ds)?;

        Ok(())
    }

    fn get(
        &self,
        agent_id: &AgentID,
    ) -> Result<Option<DataStored<Self::Identifiers>>, Self::Error> {
        debug!("storer: getting Instance ID of agent_id: {}", agent_id);

        if let Some(data) = self
            .k8s_store
            .get_opamp_data(agent_id, STORE_KEY_INSTANCE_ID)?
        {
            return Ok(Some(data));
        }

        Ok(None)
    }
}

impl Storer {
    pub fn new(k8s_store: Arc<K8sStore>) -> Self {
        Self { k8s_store }
    }
}
