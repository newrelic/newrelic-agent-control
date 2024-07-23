use crate::k8s;
use crate::k8s::store::K8sStore;
use crate::k8s::store::STORE_KEY_OPAMP_DATA_CONFIG_HASH;
use crate::opamp::hash_repository::repository::HashRepositoryError;
use crate::opamp::hash_repository::HashRepository;
use crate::opamp::remote_config_hash::Hash;
use crate::super_agent::config::AgentID;
use std::sync::Arc;
use thiserror::Error;
use tracing::debug;

#[derive(Error, Debug)]
pub enum K8sHashRepositoryError {
    #[error("k8s request failed on Config Map {0}")]
    K8sError(#[from] k8s::Error),
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
        self._save(agent_id, hash)
            .map_err(|err| HashRepositoryError::PersistError(err.to_string()))
    }

    fn get(&self, agent_id: &AgentID) -> Result<Option<Hash>, HashRepositoryError> {
        self._get(agent_id)
            .map_err(|err| HashRepositoryError::LoadError(err.to_string()))
    }
}

impl HashRepositoryConfigMap {
    fn _save(&self, agent_id: &AgentID, hash: &Hash) -> Result<(), K8sHashRepositoryError> {
        debug!("saving remote config hash of agent_id: {}", agent_id);

        self.k8s_store
            .set_opamp_data(agent_id, STORE_KEY_OPAMP_DATA_CONFIG_HASH, hash)?;
        Ok(())
    }

    fn _get(&self, agent_id: &AgentID) -> Result<Option<Hash>, K8sHashRepositoryError> {
        debug!("getting remote config hash of agent_id: {}", agent_id);

        match self
            .k8s_store
            .get_opamp_data(agent_id, STORE_KEY_OPAMP_DATA_CONFIG_HASH)?
        {
            Some(hash) => Ok(Some(hash)),
            None => Ok(None),
        }
    }
}
