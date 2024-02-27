#[cfg_attr(test, mockall_double::double)]
use super::client::SyncK8sClient;
use super::labels::Labels;
use super::Error;
use crate::super_agent::config::AgentID;
use std::sync::Arc;

/// The prefix for the ConfigMap metadata.name.
pub const CM_NAME_PREFIX: &str = "agent-data-";

/// The key used to identify the data in the Store.
pub type StoreKey = str;

pub const STORE_KEY_REMOTE_CONFIG_HASH: &StoreKey = "remote_config_hash";
pub const STORE_KEY_INSTANCE_ID: &StoreKey = "instance_id";

/// Represents a Kubernetes persistent store of Agents data such as instance id and configs.
/// The store is implemented using one ConfigMap per Agent with all the data.
pub struct K8sStore {
    k8s_client: Arc<SyncK8sClient>,
}

impl K8sStore {
    /// Creates a new K8sStore.
    pub fn new(k8s_client: Arc<SyncK8sClient>) -> Self {
        Self { k8s_client }
    }

    /// Retrieves data from an Agent store.
    /// Returns None when either is no store, the storeKey is not present or there is no data on the key.
    pub fn get<T>(&self, agent_id: &AgentID, key: &StoreKey) -> Result<Option<T>, Error>
    where
        T: serde::de::DeserializeOwned,
    {
        let configmap_name = format!("{}{}", CM_NAME_PREFIX, agent_id);

        if let Some(data) = self.k8s_client.get_configmap_key(&configmap_name, key)? {
            let ds = serde_yaml::from_str::<T>(&data)?;

            return Ok(Some(ds));
        }

        Ok(None)
    }

    /// Stores data in the specified StoreKey of an Agent store.
    pub fn set<T>(&self, agent_id: &AgentID, key: &StoreKey, data: &T) -> Result<(), Error>
    where
        T: serde::Serialize,
    {
        let data_as_string = serde_yaml::to_string(data)?;

        let configmap_name = format!("{}{}", CM_NAME_PREFIX, agent_id);

        self.k8s_client.set_configmap_key(
            &configmap_name,
            Labels::new(agent_id).get(),
            key,
            &data_as_string,
        )
    }
}

#[cfg(test)]
pub mod test {
    use super::CM_NAME_PREFIX;
    use super::{K8sStore, StoreKey};
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

    #[derive(Deserialize, Serialize, Default, Debug, PartialEq)]
    pub struct DataToBeStored {
        pub test: String,
    }

    #[test]
    fn test_input_parameters_dependencies() {
        // In this tests we are checking that the parameters are passed as expected and that cm names are built in the proper way
        // The output of the commands are checked in following tests.
        let mut k8s_client = MockSyncK8sClient::default();
        k8s_client
            .expect_get_configmap_key()
            .once()
            .with(
                predicate::function(|name| {
                    name == format!("{}{}", CM_NAME_PREFIX, AGENT_NAME).as_str()
                }),
                predicate::function(|key| key == STORE_KEY_TEST),
            )
            .returning(move |_, _| Ok(None));
        k8s_client
            .expect_set_configmap_key()
            .once()
            .with(
                predicate::function(|name| {
                    name == format!("{}{}", CM_NAME_PREFIX, AGENT_NAME).as_str()
                }),
                predicate::function(|key| {
                    key == &Labels::new(&AgentID::new(AGENT_NAME).unwrap()).get()
                }),
                predicate::function(|key| key == STORE_KEY_TEST),
                predicate::function(|ds| ds == DATA_STORED),
            )
            .returning(move |_, _, _, _| Ok(()));

        let k8s_store = K8sStore::new(Arc::new(k8s_client));

        let _ = k8s_store.get::<DataToBeStored>(&AgentID::new(AGENT_NAME).unwrap(), STORE_KEY_TEST);

        let _ = k8s_store.set(
            &AgentID::new(AGENT_NAME).unwrap(),
            STORE_KEY_TEST,
            &DataToBeStored {
                test: "foo".to_string(),
            },
        );
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
            .get::<DataToBeStored>(&AgentID::new(AGENT_NAME).unwrap(), STORE_KEY_TEST)
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
            .get::<DataToBeStored>(&AgentID::new(AGENT_NAME).unwrap(), STORE_KEY_TEST)
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
            .get::<DataToBeStored>(&AgentID::new(AGENT_NAME).unwrap(), STORE_KEY_TEST)
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

        let id = k8s_store.set(
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
        let id = k8s_store.set(
            &AgentID::new(AGENT_NAME).unwrap(),
            STORE_KEY_TEST,
            &DataToBeStored::default(),
        );
        assert!(id.is_ok())
    }
}
