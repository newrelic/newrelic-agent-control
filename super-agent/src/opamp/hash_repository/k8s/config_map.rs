use crate::k8s;
use crate::k8s::store::K8sStore;
use crate::k8s::store::STORE_KEY_REMOTE_CONFIG_HASH;
use crate::opamp::hash_repository::HashRepository;
use crate::opamp::remote_config_hash::Hash;
use crate::super_agent::config::AgentID;
use std::sync::Arc;
use thiserror::Error;
use tracing::debug;

#[derive(Error, Debug)]
pub enum HashRepositoryError {
    #[error("failed to persist on Config Map {0}")]
    FailedToPersistK8s(#[from] k8s::Error),

    #[error("entry not found")]
    NotFound,
}

pub struct HashRepositoryConfigMap {
    k8s_store: Arc<K8sStore>,
}

impl HashRepositoryConfigMap {
    pub fn new(k8s_store: Arc<K8sStore>) -> Self {
        Self { k8s_store }
    }
}

impl HashRepository for HashRepositoryConfigMap {
    fn save(&self, agent_id: &AgentID, hash: &Hash) -> Result<(), HashRepositoryError> {
        debug!("saving remote config hash of agent_id: {}", agent_id);

        self.k8s_store
            .set(agent_id, STORE_KEY_REMOTE_CONFIG_HASH, hash)?;
        Ok(())
    }

    fn get(&self, agent_id: &AgentID) -> Result<Hash, HashRepositoryError> {
        debug!("getting remote config hash of agent_id: {}", agent_id);

        if let Some(hash) = self.k8s_store.get(agent_id, STORE_KEY_REMOTE_CONFIG_HASH)? {
            return Ok(hash);
        }

        Err(HashRepositoryError::NotFound)
    }
}
