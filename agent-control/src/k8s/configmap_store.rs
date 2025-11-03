use serde::Serialize;
use serde::de::DeserializeOwned;

use super::Error;
#[cfg_attr(test, mockall_double::double)]
use super::client::SyncK8sClient;
use super::labels::Labels;
use crate::agent_control::agent_id::AgentID;
use crate::agent_control::defaults::{FOLDER_NAME_FLEET_DATA, FOLDER_NAME_LOCAL_DATA};
use crate::k8s;
use crate::opamp::data_store::{OpAMPDataStore, StoreKey};
use std::sync::Arc;

/// Represents a Kubernetes persistent store of Agents data such as instance id and configs.
/// The store is implemented using one ConfigMap per Agent with all the data.
pub struct ConfigMapStore {
    k8s_client: Arc<SyncK8sClient>,
    namespace: String,
}

impl ConfigMapStore {
    /// Creates a new K8sStore.
    pub fn new(k8s_client: Arc<SyncK8sClient>, namespace: String) -> Self {
        Self {
            k8s_client,
            namespace,
        }
    }

    pub fn build_cm_name(agent_id: &AgentID, prefix: &str) -> String {
        format!("{prefix}-{agent_id}")
    }

    /// Retrieves data from an Agent store.
    /// Returns None when either is no store, the storeKey is not present or there is no data on the key.
    fn get<T>(&self, agent_id: &AgentID, prefix: &str, key: &StoreKey) -> Result<Option<T>, Error>
    where
        T: serde::de::DeserializeOwned,
    {
        let configmap_name = ConfigMapStore::build_cm_name(agent_id, prefix);
        if let Some(data) =
            self.k8s_client
                .get_configmap_key(&configmap_name, self.namespace.as_str(), key)?
        {
            let ds = serde_yaml::from_str::<T>(&data)?;

            return Ok(Some(ds));
        }

        Ok(None)
    }
}

impl OpAMPDataStore for ConfigMapStore {
    type Error = k8s::Error;
    /// get_opamp_data is used to get data from CMs storing data related with opamp:
    /// Instance IDs, hashes, and remote configs.
    fn get_opamp_data<T>(&self, agent_id: &AgentID, key: &str) -> Result<Option<T>, Self::Error>
    where
        T: DeserializeOwned,
    {
        self.get(agent_id, FOLDER_NAME_FLEET_DATA, key)
    }

    /// get_local_data is used to get data from CMs storing local configurations. I.e. all the CMs
    /// created by the agent-control-deployment chart.
    fn get_local_data<T>(&self, agent_id: &AgentID, key: &str) -> Result<Option<T>, Self::Error>
    where
        T: DeserializeOwned,
    {
        self.get(agent_id, FOLDER_NAME_LOCAL_DATA, key)
    }

    /// Stores data in the specified StoreKey of an Agent store.
    fn set_opamp_data<T>(&self, agent_id: &AgentID, key: &str, data: &T) -> Result<(), Self::Error>
    where
        T: Serialize,
    {
        let data_as_string = serde_yaml::to_string(data)?;
        let configmap_name = ConfigMapStore::build_cm_name(agent_id, FOLDER_NAME_FLEET_DATA);
        self.k8s_client.set_configmap_key(
            &configmap_name,
            self.namespace.as_str(),
            Labels::new(agent_id).get(),
            key,
            &data_as_string,
        )
    }

    /// Delete data in the specified StoreKey of an Agent store.
    fn delete_opamp_data(&self, agent_id: &AgentID, key: &str) -> Result<(), Self::Error> {
        let configmap_name = ConfigMapStore::build_cm_name(agent_id, FOLDER_NAME_FLEET_DATA);
        self.k8s_client
            .delete_configmap_key(&configmap_name, self.namespace.as_str(), key)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::agent_control::agent_id::AgentID;
    use crate::agent_control::defaults::{FOLDER_NAME_FLEET_DATA, FOLDER_NAME_LOCAL_DATA};
    use crate::k8s::client::MockSyncK8sClient;
    use crate::k8s::error::K8sError;
    use crate::k8s::labels::Labels;
    use mockall::predicate;
    use serde::{Deserialize, Serialize};
    use std::sync::Arc;

    const AGENT_NAME: &str = "agent1";
    const DATA_STORED: &str = "test: foo\n";
    pub const STORE_KEY_TEST: &StoreKey = "data_to_be_stored";
    const TEST_NAMESPACE: &str = "test-namespace";
    pub const PREFIX_TEST: &StoreKey = "prefix";

    #[derive(Deserialize, Serialize, Default, Debug, PartialEq)]
    pub struct DataToBeStored {
        pub test: String,
    }

    #[test]
    fn test_opamp_set_delete_input_parameters_dependencies() {
        // In this test we are checking that the parameters are passed as expected and that cm names are built in the proper way
        // The output of the commands are checked in following tests.
        let mut k8s_client = MockSyncK8sClient::default();
        let agent_id = AgentID::try_from(AGENT_NAME).unwrap();

        k8s_client
            .expect_set_configmap_key()
            .once()
            .with(
                predicate::eq(ConfigMapStore::build_cm_name(
                    &agent_id,
                    FOLDER_NAME_FLEET_DATA,
                )),
                predicate::eq(TEST_NAMESPACE),
                predicate::eq(Labels::new(&AgentID::try_from(AGENT_NAME).unwrap()).get()),
                predicate::eq(STORE_KEY_TEST),
                predicate::eq(DATA_STORED),
            )
            .returning(move |_, _, _, _, _| Ok(()));
        k8s_client
            .expect_delete_configmap_key()
            .once()
            .with(
                predicate::eq(ConfigMapStore::build_cm_name(
                    &agent_id,
                    FOLDER_NAME_FLEET_DATA,
                )),
                predicate::eq(TEST_NAMESPACE),
                predicate::eq(STORE_KEY_TEST),
            )
            .returning(move |_, _, _| Ok(()));

        let k8s_store = ConfigMapStore::new(Arc::new(k8s_client), TEST_NAMESPACE.to_string());

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
        let agent_id = &AgentID::try_from(AGENT_NAME).unwrap();

        k8s_client
            .expect_get_configmap_key()
            .with(
                predicate::eq(ConfigMapStore::build_cm_name(
                    agent_id,
                    FOLDER_NAME_FLEET_DATA,
                )),
                predicate::eq(TEST_NAMESPACE),
                predicate::eq(STORE_KEY_TEST),
            )
            .returning(move |_, _, _| Ok(Some(DATA_STORED.to_string())));

        _ = ConfigMapStore::new(Arc::new(k8s_client), TEST_NAMESPACE.to_string())
            .get_opamp_data::<DataToBeStored>(agent_id, STORE_KEY_TEST);

        // local
        let mut k8s_client = MockSyncK8sClient::default();
        k8s_client
            .expect_get_configmap_key()
            .with(
                predicate::eq(ConfigMapStore::build_cm_name(
                    agent_id,
                    FOLDER_NAME_LOCAL_DATA,
                )),
                predicate::eq(TEST_NAMESPACE),
                predicate::always(),
            )
            .returning(move |_, _, _| Ok(Some(DATA_STORED.to_string())));

        _ = ConfigMapStore::new(Arc::new(k8s_client), TEST_NAMESPACE.to_string())
            .get_local_data::<DataToBeStored>(
                &AgentID::try_from(AGENT_NAME).unwrap(),
                STORE_KEY_TEST,
            );
    }

    #[test]
    fn test_get_error() {
        let mut k8s_client = MockSyncK8sClient::default();
        k8s_client
            .expect_get_configmap_key()
            .once()
            .returning(move |_, _, _| Err(K8sError::KubeRs(Box::new(kube::Error::TlsRequired))));

        let k8s_store = ConfigMapStore::new(Arc::new(k8s_client), TEST_NAMESPACE.to_string());

        k8s_store
            .get::<DataToBeStored>(
                &AgentID::try_from(AGENT_NAME).unwrap(),
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
            .returning(move |_, _, _| Ok(None));

        let k8s_store = ConfigMapStore::new(Arc::new(k8s_client), TEST_NAMESPACE.to_string());

        let data = k8s_store
            .get::<DataToBeStored>(
                &AgentID::try_from(AGENT_NAME).unwrap(),
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
            .returning(move |_, _, _| Ok(Some(DATA_STORED.to_string())));
        let k8s_store = ConfigMapStore::new(Arc::new(k8s_client), TEST_NAMESPACE.to_string());

        let data = k8s_store
            .get::<DataToBeStored>(
                &AgentID::try_from(AGENT_NAME).unwrap(),
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
            .returning(move |_, _, _, _, _| {
                Err(K8sError::KubeRs(Box::new(kube::Error::TlsRequired)))
            });
        let k8s_store = ConfigMapStore::new(Arc::new(k8s_client), TEST_NAMESPACE.to_string());

        let id = k8s_store.set_opamp_data(
            &AgentID::try_from(AGENT_NAME).unwrap(),
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
            .returning(move |_, _, _, _, _| Ok(()));
        let k8s_store = ConfigMapStore::new(Arc::new(k8s_client), TEST_NAMESPACE.to_string());
        let id = k8s_store.set_opamp_data(
            &AgentID::try_from(AGENT_NAME).unwrap(),
            STORE_KEY_TEST,
            &DataToBeStored::default(),
        );
        assert!(id.is_ok())
    }

    #[test]
    fn test_build_cm_name() {
        let agent_id = AgentID::try_from(AGENT_NAME).unwrap();
        assert_eq!(
            "prefix-agent1",
            ConfigMapStore::build_cm_name(&agent_id, PREFIX_TEST)
        );
    }
}
