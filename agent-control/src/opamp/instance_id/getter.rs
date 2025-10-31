use crate::agent_control::agent_id::AgentID;
use crate::agent_control::defaults::STORE_KEY_INSTANCE_ID;
use crate::k8s;
use crate::opamp::data_store::OpAMPDataStore;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::io;
use std::sync::Arc;
use thiserror::Error;
use tracing::debug;

use super::{InstanceID, definition::InstanceIdentifiers};

#[derive(Error, Debug)]
pub enum GetterError {
    #[error("host I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("k8s error: {0}")]
    K8s(#[from] k8s::Error),

    #[cfg(test)]
    #[error("mock getter error")]
    MockGetterError,
}

pub struct InstanceIDWithIdentifiersGetter<D, I>
where
    D: OpAMPDataStore,
    I: InstanceIdentifiers + Serialize + DeserializeOwned + 'static,
{
    opamp_data_store: Arc<D>,
    identifiers: I,
}

impl<D, I> InstanceIDWithIdentifiersGetter<D, I>
where
    D: OpAMPDataStore,
    I: InstanceIdentifiers + Serialize + DeserializeOwned + 'static,
{
    pub fn new(opamp_data_store: Arc<D>, identifiers: I) -> Self {
        Self {
            opamp_data_store,
            identifiers,
        }
    }

    pub fn get(&self, agent_id: &AgentID) -> Result<InstanceID, GetterError> {
        debug!(target_agent_id = %agent_id, "retrieving instance id");
        let data = self
            .opamp_data_store
            .get_opamp_data::<DataStored<I>>(agent_id, STORE_KEY_INSTANCE_ID)
            .map_err(Into::into)?;

        match data {
            None => {
                debug!("storer returned no data");
            }
            Some(d) if d.identifiers == self.identifiers => return Ok(d.instance_id),
            Some(d) => debug!(
                target_agent_id = %agent_id,
                "stored data had different identifiers {:?}!={:?}", d.identifiers, self.identifiers
            ),
        }

        let new_data = DataStored {
            instance_id: InstanceID::create(),
            identifiers: self.identifiers.clone(),
        };

        debug!(target_agent_id = %agent_id, "persisting instance id {}", new_data.instance_id);
        self.opamp_data_store
            .set_opamp_data(agent_id, STORE_KEY_INSTANCE_ID, &new_data)
            .map_err(Into::into)?;

        Ok(new_data.instance_id)
    }
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Clone)]
#[serde(bound = "I: InstanceIdentifiers")]
pub struct DataStored<I>
where
    I: InstanceIdentifiers,
{
    pub instance_id: InstanceID,
    pub identifiers: I,
}

#[cfg(test)]
pub mod tests {
    use std::sync::Arc;

    use super::*;

    use crate::opamp::data_store::tests::{MockDataStoreError, MockOpAMPDataStore};
    use crate::opamp::instance_id::definition::tests::MockIdentifiers;
    use crate::opamp::instance_id::getter::{DataStored, InstanceIDWithIdentifiersGetter};
    use mockall::predicate;
    use opamp_client::operation::instance_uid::InstanceUid;

    const AGENT_NAME: &str = "agent1";

    #[test]
    fn test_not_found() {
        let mut mock = MockOpAMPDataStore::new();

        let agent_id = AgentID::try_from(AGENT_NAME).unwrap();
        mock.expect_get_opamp_data::<DataStored<MockIdentifiers>>()
            .once()
            .with(
                predicate::eq(agent_id.clone()),
                predicate::eq(STORE_KEY_INSTANCE_ID),
            )
            .returning(|_, _| Ok(None));
        mock.expect_set_opamp_data::<DataStored<MockIdentifiers>>()
            .once()
            .with(
                predicate::eq(agent_id.clone()),
                predicate::eq(STORE_KEY_INSTANCE_ID),
                predicate::always(),
            )
            .returning(|_, _, _| Ok(()));
        let getter =
            InstanceIDWithIdentifiersGetter::new(Arc::new(mock), MockIdentifiers::default());
        let res = getter.get(&AgentID::try_from(AGENT_NAME).unwrap());

        assert!(res.is_ok());
    }

    #[test]
    fn test_error_get() {
        let mut mock = MockOpAMPDataStore::new();

        let agent_id = AgentID::try_from(AGENT_NAME).unwrap();
        mock.expect_get_opamp_data::<DataStored<MockIdentifiers>>()
            .once()
            .with(
                predicate::eq(agent_id.clone()),
                predicate::eq(STORE_KEY_INSTANCE_ID),
            )
            .returning(|_, _| Err(MockDataStoreError));
        let getter =
            InstanceIDWithIdentifiersGetter::new(Arc::new(mock), MockIdentifiers::default());
        let res = getter.get(&AgentID::try_from(AGENT_NAME).unwrap());

        assert!(res.is_err());
    }

    #[test]
    fn test_error_set() {
        let mut mock = MockOpAMPDataStore::new();

        let agent_id = AgentID::try_from(AGENT_NAME).unwrap();
        mock.expect_get_opamp_data::<DataStored<MockIdentifiers>>()
            .once()
            .with(
                predicate::eq(agent_id.clone()),
                predicate::eq(STORE_KEY_INSTANCE_ID),
            )
            .returning(|_, _| Ok(None));
        mock.expect_set_opamp_data::<DataStored<MockIdentifiers>>()
            .once()
            .with(
                predicate::eq(agent_id.clone()),
                predicate::eq(STORE_KEY_INSTANCE_ID),
                predicate::always(),
            )
            .returning(|_, _, _| Err(MockDataStoreError));

        let getter =
            InstanceIDWithIdentifiersGetter::new(Arc::new(mock), MockIdentifiers::default());
        let res = getter.get(&AgentID::try_from(AGENT_NAME).unwrap());

        assert!(res.is_err());
    }

    #[test]
    fn test_instance_id_already_present() {
        let mut mock = MockOpAMPDataStore::new();
        let instance_id = InstanceID::create();
        let agent_id = AgentID::try_from(AGENT_NAME).unwrap();

        let instance_id_clone = instance_id.clone();
        mock.expect_get_opamp_data()
            .once()
            .with(
                predicate::eq(agent_id.clone()),
                predicate::eq(STORE_KEY_INSTANCE_ID),
            )
            .return_once(move |_, _| {
                Ok(Some(DataStored {
                    instance_id: instance_id_clone,
                    identifiers: MockIdentifiers::default(),
                }))
            });
        let getter =
            InstanceIDWithIdentifiersGetter::new(Arc::new(mock), MockIdentifiers::default());
        let res = getter.get(&AgentID::try_from(AGENT_NAME).unwrap());

        assert!(res.is_ok());
        assert_eq!(instance_id, res.unwrap());
    }

    #[test]
    fn test_instance_id_present_but_different_identifiers() {
        let mut mock = MockOpAMPDataStore::new();
        let instance_id = InstanceID::create();
        let agent_id = AgentID::try_from(AGENT_NAME).unwrap();

        let instance_id_clone = instance_id.clone();
        mock.expect_get_opamp_data()
            .once()
            .with(
                predicate::eq(agent_id.clone()),
                predicate::eq(STORE_KEY_INSTANCE_ID),
            )
            .return_once(move |_, _| {
                Ok(Some(DataStored {
                    instance_id: instance_id_clone,
                    identifiers: get_different_identifier(),
                }))
            });
        mock.expect_set_opamp_data::<DataStored<MockIdentifiers>>()
            .once()
            .with(
                predicate::eq(agent_id.clone()),
                predicate::eq(STORE_KEY_INSTANCE_ID),
                predicate::always(),
            )
            .returning(|_, _, _| Ok(()));
        let getter =
            InstanceIDWithIdentifiersGetter::new(Arc::new(mock), MockIdentifiers::default());
        let res = getter.get(&AgentID::try_from(AGENT_NAME).unwrap());

        assert!(res.is_ok());
        assert_ne!(instance_id, res.unwrap());
    }

    #[test]
    fn test_uuid() {
        let uuid_as_str = "018FF38D01B37796B2C81C8069BC6ADF";
        // Crete InstanceID from string
        let id: InstanceID = serde_yaml::from_str(uuid_as_str).unwrap();
        // Convert instanceID to OpAMP instanceUid
        let instance_uid = InstanceUid::from(id.clone());
        // Get the instanceID back from the corresponding bytes
        let id_from_bytes: InstanceID = InstanceUid::try_from(Vec::<u8>::from(instance_uid))
            .unwrap()
            .into();
        assert_eq!(id, id_from_bytes);
        assert_eq!(uuid_as_str, format!("{id_from_bytes}"));
    }

    fn get_different_identifier() -> MockIdentifiers {
        MockIdentifiers(1)
    }
}
