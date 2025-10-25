use std::{io::Error, path::Path};

use serde::{Serialize, de::DeserializeOwned};

use crate::agent_control::agent_id::AgentID;

struct FileStore;

// Proposed API
impl FileStore {
    pub fn new() -> Self {
        FileStore
    }

    pub fn get_opamp_data<T>(&self, agent_id: &AgentID, key: &Path) -> Result<Option<T>, Error>
    where
        T: DeserializeOwned,
    {
        // self.get(agent_id, FOLDER_NAME_FLEET_DATA, key)
        unimplemented!();
    }

    pub fn get_local_data<T>(&self, agent_id: &AgentID, key: &Path) -> Result<Option<T>, Error>
    where
        T: DeserializeOwned,
    {
        // self.get(agent_id, FOLDER_NAME_LOCAL_DATA, key)
        unimplemented!();
    }

    /// Retrieves data from an Agent store.
    /// Returns None when either is no store, the storeKey is not present or there is no data on the key.
    fn get<T>(&self, agent_id: &AgentID, prefix: &str, key: &Path) -> Result<Option<T>, Error>
    where
        T: DeserializeOwned,
    {
        // let _read_guard = self.rw_lock.read().unwrap();

        // let configmap_name = K8sStore::build_cm_name(agent_id, prefix);
        // if let Some(data) =
        //     self.k8s_client
        //         .get_configmap_key(&configmap_name, self.namespace.as_str(), key)?
        // {
        //     let ds = serde_yaml::from_str::<T>(&data)?;

        //     return Ok(Some(ds));
        // }

        // Ok(None)
        unimplemented!();
    }

    /// Stores data in the specified StoreKey of an Agent store.
    pub fn set_opamp_data<T>(&self, agent_id: &AgentID, key: &Path, data: &T) -> Result<(), Error>
    where
        T: Serialize,
    {
        // #[allow(clippy::readonly_write_lock)]
        // let _write_guard = self.rw_lock.write().unwrap();

        // let data_as_string = serde_yaml::to_string(data)?;
        // let configmap_name = K8sStore::build_cm_name(agent_id, FOLDER_NAME_FLEET_DATA);
        // self.k8s_client.set_configmap_key(
        //     &configmap_name,
        //     self.namespace.as_str(),
        //     Labels::new(agent_id).get(),
        //     key,
        //     &data_as_string,
        // )
        unimplemented!();
    }

    /// Delete data in the specified StoreKey of an Agent store.
    pub fn delete_opamp_data(&self, agent_id: &AgentID, key: &Path) -> Result<(), Error> {
        // #[allow(clippy::readonly_write_lock)]
        // let _write_guard = self.rw_lock.write().unwrap();

        // let configmap_name = K8sStore::build_cm_name(agent_id, FOLDER_NAME_FLEET_DATA);
        // self.k8s_client
        //     .delete_configmap_key(&configmap_name, self.namespace.as_str(), key)
        unimplemented!();
    }

    pub fn build_cm_name(agent_id: &AgentID, prefix: &str) -> String {
        format!("{prefix}-{agent_id}")
    }
}
