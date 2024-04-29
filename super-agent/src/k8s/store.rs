#[cfg_attr(test, mockall_double::double)]
use super::client::SyncK8sClient;
use super::labels::Labels;
use super::Error;
use crate::super_agent::config::AgentID;
use std::sync::{Arc, RwLock};

/// The prefixes for the ConfigMap name.
/// The cm having CM_NAME_LOCAL_DATA_PREFIX stores all the config that are "local",
/// the SA treats those CM as read-only.
pub const CM_NAME_LOCAL_DATA_PREFIX: &str = "local-data-";
/// The cm having CM_NAME_OPAMP_DATA_PREFIX as prefix stores all the data related with opamp:
/// ULIDs, hashes, and remote configs. The Sa reads and writes those CMs.
pub const CM_NAME_OPAMP_DATA_PREFIX: &str = "opamp-data-";

/// The key used to identify the data in the Store.
pub type StoreKey = str;

pub const STORE_KEY_LOCAL_DATA_CONFIG: &StoreKey = "local_config";
pub const STORE_KEY_OPAMP_DATA_CONFIG: &StoreKey = "remote_config";
pub const STORE_KEY_OPAMP_DATA_CONFIG_HASH: &StoreKey = "remote_config_hash";
pub const STORE_KEY_INSTANCE_ID: &StoreKey = "instance_id";

/// Represents a Kubernetes persistent store of Agents data such as instance id and configs.
/// The store is implemented using one ConfigMap per Agent with all the data.
pub struct K8sStore {
    k8s_client: Arc<SyncK8sClient>,
    rw_lock: RwLock<()>,
}

impl K8sStore {
    /// Creates a new K8sStore.
    pub fn new(k8s_client: Arc<SyncK8sClient>) -> Self {
        Self {
            k8s_client,
            rw_lock: RwLock::new(()),
        }
    }

    /// get_opamp_data is used to get data from CMs storing data related with opamp:
    /// ULIDs, hashes, and remote configs.
    pub fn get_opamp_data<T>(&self, agent_id: &AgentID, key: &StoreKey) -> Result<Option<T>, Error>
    where
        T: serde::de::DeserializeOwned,
    {
        self.get(agent_id, CM_NAME_OPAMP_DATA_PREFIX, key)
    }

    /// get_local_data is used to get data from CMs storing local configurations. I.e. all the CMs
    /// created by the super-agent-deployment chart.
    pub fn get_local_data<T>(&self, agent_id: &AgentID, key: &StoreKey) -> Result<Option<T>, Error>
    where
        T: serde::de::DeserializeOwned,
    {
        self.get(agent_id, CM_NAME_LOCAL_DATA_PREFIX, key)
    }

    /// Retrieves data from an Agent store.
    /// Returns None when either is no store, the storeKey is not present or there is no data on the key.
    fn get<T>(&self, agent_id: &AgentID, prefix: &str, key: &StoreKey) -> Result<Option<T>, Error>
    where
        T: serde::de::DeserializeOwned,
    {
        let _read_guard = self.rw_lock.read().unwrap();

        let configmap_name = K8sStore::build_cm_name(agent_id, prefix);
        if let Some(data) = self.k8s_client.get_configmap_key(&configmap_name, key)? {
            let ds = serde_yaml::from_str::<T>(&data)?;

            return Ok(Some(ds));
        }

        Ok(None)
    }

    /// Stores data in the specified StoreKey of an Agent store.
    pub fn set_opamp_data<T>(
        &self,
        agent_id: &AgentID,
        key: &StoreKey,
        data: &T,
    ) -> Result<(), Error>
    where
        T: serde::Serialize,
    {
        // clippy complains because we are not changing the lock's content
        // TODO: check RwLock is being used efficiently for this use-case.
        #[allow(clippy::readonly_write_lock)]
        let _write_guard = self.rw_lock.write().unwrap();

        let data_as_string = serde_yaml::to_string(data)?;
        let configmap_name = K8sStore::build_cm_name(agent_id, CM_NAME_OPAMP_DATA_PREFIX);
        self.k8s_client.set_configmap_key(
            &configmap_name,
            Labels::new(agent_id).get(),
            key,
            &data_as_string,
        )
    }

    /// Delete data in the specified StoreKey of an Agent store.
    pub fn delete_opamp_data(&self, agent_id: &AgentID, key: &StoreKey) -> Result<(), Error> {
        // clippy complains because we are not changing the lock's content
        // TODO: check RwLock is being used efficiently for this use-case.
        #[allow(clippy::readonly_write_lock)]
        let _write_guard = self.rw_lock.write().unwrap();

        let configmap_name = K8sStore::build_cm_name(agent_id, CM_NAME_OPAMP_DATA_PREFIX);
        self.k8s_client.delete_configmap_key(&configmap_name, key)
    }

    fn build_cm_name(agent_id: &AgentID, prefix: &str) -> String {
        format!("{}{}", prefix, agent_id)
    }
}

#[cfg(test)]
pub mod test {
    use super::{K8sStore, StoreKey};
    use super::{CM_NAME_LOCAL_DATA_PREFIX, CM_NAME_OPAMP_DATA_PREFIX};
    use crate::k8s::client::MockSyncK8sClient;
    use crate::k8s::error::K8sError;
    use crate::k8s::labels::Labels;
    use crate::super_agent::config::AgentID;
    use mockall::predicate;
    use serde::{Deserialize, Serialize};
    use std::sync::Arc;

    const AGENT_NAME: &str = "agent1";
    const DATA_STORED: &str = "test: foo\n";
    pub const STORE_KEY_TEST: &StoreKey = "data_to_be_stored";

    pub const PREFIX_TEST: &StoreKey = "prefix-";

    #[derive(Deserialize, Serialize, Default, Debug, PartialEq)]
    pub struct DataToBeStored {
        pub test: String,
    }

    #[test]
    fn test_opamp_set_delete_input_parameters_dependencies() {
        // In this test we are checking that the parameters are passed as expected and that cm names are built in the proper way
        // The output of the commands are checked in following tests.
        let mut k8s_client = MockSyncK8sClient::default();
        let agent_id = AgentID::new(AGENT_NAME).unwrap();

        k8s_client
            .expect_set_configmap_key()
            .once()
            .with(
                predicate::eq(K8sStore::build_cm_name(
                    &agent_id,
                    CM_NAME_OPAMP_DATA_PREFIX,
                )),
                predicate::eq(Labels::new(&AgentID::new(AGENT_NAME).unwrap()).get()),
                predicate::eq(STORE_KEY_TEST),
                predicate::eq(DATA_STORED),
            )
            .returning(move |_, _, _, _| Ok(()));
        k8s_client
            .expect_delete_configmap_key()
            .once()
            .with(
                predicate::eq(K8sStore::build_cm_name(
                    &agent_id,
                    CM_NAME_OPAMP_DATA_PREFIX,
                )),
                predicate::eq(STORE_KEY_TEST),
            )
            .returning(move |_, _| Ok(()));

        let k8s_store = K8sStore::new(Arc::new(k8s_client));

        let _ = k8s_store.set_opamp_data(
            &agent_id,
            STORE_KEY_TEST,
            &DataToBeStored {
                test: "foo".to_string(),
            },
        );

        let _ = k8s_store.delete_opamp_data(&agent_id, STORE_KEY_TEST);
    }

    #[test]
    fn test_get_input_parameters_dependencies() {
        // remote
        let mut k8s_client = MockSyncK8sClient::default();
        let agent_id = &AgentID::new(AGENT_NAME).unwrap();

        k8s_client
            .expect_get_configmap_key()
            .with(
                predicate::eq(K8sStore::build_cm_name(
                    &agent_id,
                    CM_NAME_OPAMP_DATA_PREFIX,
                )),
                predicate::eq(STORE_KEY_TEST),
            )
            .returning(move |_, _| Ok(Some(DATA_STORED.to_string())));

        _ = K8sStore::new(Arc::new(k8s_client))
            .get_opamp_data::<DataToBeStored>(agent_id, STORE_KEY_TEST);

        // local
        let mut k8s_client = MockSyncK8sClient::default();
        k8s_client
            .expect_get_configmap_key()
            .with(
                predicate::eq(K8sStore::build_cm_name(
                    &agent_id,
                    CM_NAME_LOCAL_DATA_PREFIX,
                )),
                predicate::always(),
            )
            .returning(move |_, _| Ok(Some(DATA_STORED.to_string())));

        _ = K8sStore::new(Arc::new(k8s_client))
            .get_local_data::<DataToBeStored>(&AgentID::new(AGENT_NAME).unwrap(), STORE_KEY_TEST);
    }

    #[test]
    fn test_get_error() {
        let mut k8s_client = MockSyncK8sClient::default();
        k8s_client
            .expect_get_configmap_key()
            .once()
            .returning(move |_, _| Err(K8sError::Generic(kube::Error::TlsRequired)));

        let k8s_store = K8sStore::new(Arc::new(k8s_client));

        k8s_store
            .get::<DataToBeStored>(
                &AgentID::new(AGENT_NAME).unwrap(),
                PREFIX_TEST,
                STORE_KEY_TEST,
            )
            .unwrap_err();
    }

    #[test]
    fn test_get_not_found() {
        let mut k8s_client = MockSyncK8sClient::default();
        k8s_client
            .expect_get_configmap_key()
            .once()
            .returning(move |_, _| Ok(None));

        let k8s_store = K8sStore::new(Arc::new(k8s_client));

        let data = k8s_store
            .get::<DataToBeStored>(
                &AgentID::new(AGENT_NAME).unwrap(),
                PREFIX_TEST,
                STORE_KEY_TEST,
            )
            .unwrap();
        assert!(data.is_none());
    }

    #[test]
    fn test_get_found_data() {
        let mut k8s_client = MockSyncK8sClient::default();
        k8s_client
            .expect_get_configmap_key()
            .once()
            .returning(move |_, _| Ok(Some(DATA_STORED.to_string())));
        let k8s_store = K8sStore::new(Arc::new(k8s_client));

        let data = k8s_store
            .get::<DataToBeStored>(
                &AgentID::new(AGENT_NAME).unwrap(),
                PREFIX_TEST,
                STORE_KEY_TEST,
            )
            .unwrap();
        assert_eq!(
            data.unwrap(),
            DataToBeStored {
                test: "foo".to_string()
            }
        );
    }

    #[test]
    fn test_set_error() {
        let mut k8s_client = MockSyncK8sClient::default();
        k8s_client
            .expect_set_configmap_key()
            .once()
            .returning(move |_, _, _, _| Err(K8sError::Generic(kube::Error::TlsRequired)));
        let k8s_store = K8sStore::new(Arc::new(k8s_client));

        let id = k8s_store.set_opamp_data(
            &AgentID::new(AGENT_NAME).unwrap(),
            STORE_KEY_TEST,
            &DataToBeStored::default(),
        );
        assert!(id.is_err())
    }

    #[test]
    fn test_set_succeeded() {
        let mut k8s_client = MockSyncK8sClient::default();
        k8s_client
            .expect_set_configmap_key()
            .once()
            .returning(move |_, _, _, _| Ok(()));
        let k8s_store = K8sStore::new(Arc::new(k8s_client));
        let id = k8s_store.set_opamp_data(
            &AgentID::new(AGENT_NAME).unwrap(),
            STORE_KEY_TEST,
            &DataToBeStored::default(),
        );
        assert!(id.is_ok())
    }

    #[test]
    fn test_build_cm_name() {
        let agent_id = AgentID::new(AGENT_NAME).unwrap();
        assert_eq!(
            "prefix-agent1",
            K8sStore::build_cm_name(&agent_id, PREFIX_TEST)
        );
    }
}
