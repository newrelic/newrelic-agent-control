use super::{GetterError, Identifiers};
use crate::{opamp::instance_id::storer::InstanceIDStorer, super_agent::config::AgentID};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use thiserror::Error;
use tracing::debug;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum InstanceIDError {
    #[error("invalid InstanceID format: `{0}`")]
    InvalidFormat(String),
}

// InstanceID holds the to_string of Uuid assigned to a Agent
#[derive(Debug, Deserialize, Serialize, PartialEq, Clone, Eq, Hash)]
pub struct InstanceID(Uuid);

impl InstanceID {
    // Creates a new instanceID with a random value. Use try_from methods
    // to build this struct with a static value.
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl TryFrom<String> for InstanceID {
    type Error = InstanceIDError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_from(value.as_str())
    }
}

impl TryFrom<Vec<u8>> for InstanceID {
    type Error = InstanceIDError;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        let uuid: Uuid = value
            .try_into()
            .map_err(|e: uuid::Error| InstanceIDError::InvalidFormat(e.to_string()))?;

        Ok(Self(uuid))
    }
}

impl TryFrom<&str> for InstanceID {
    type Error = InstanceIDError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Ok(Self(Uuid::parse_str(value).map_err(|e| {
            InstanceIDError::InvalidFormat(e.to_string())
        })?))
    }
}

impl From<InstanceID> for Vec<u8> {
    fn from(val: InstanceID) -> Self {
        val.0.into()
    }
}

impl Display for InstanceID {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// IDGetter returns an InstanceID for a specific agentID.
pub trait InstanceIDGetter {
    fn get(&self, agent_id: &AgentID) -> Result<InstanceID, GetterError>;
}

pub struct InstanceIDWithIdentifiersGetter<S>
where
    S: InstanceIDStorer,
{
    storer: S,
    identifiers: Identifiers,
}

impl<S> InstanceIDWithIdentifiersGetter<S>
where
    S: InstanceIDStorer,
{
    pub fn new(storer: S, identifiers: Identifiers) -> Self {
        Self {
            storer,
            identifiers,
        }
    }

    pub fn with_identifiers(self, identifiers: Identifiers) -> Self {
        Self::new(self.storer, identifiers)
    }
}

impl<S> InstanceIDGetter for InstanceIDWithIdentifiersGetter<S>
where
    S: InstanceIDStorer,
{
    fn get(&self, agent_id: &AgentID) -> Result<InstanceID, GetterError> {
        debug!("retrieving instance id");
        let data = self.storer.get(agent_id)?;

        match data {
            None => {
                debug!("storer returned no data");
            }
            Some(d) if d.identifiers == self.identifiers => return Ok(d.instance_id),
            Some(d) => debug!(
                "stored data had different identifiers {:?}!={:?}",
                d.identifiers, self.identifiers
            ),
        }

        let new_data = DataStored {
            instance_id: InstanceID::new(),
            identifiers: self.identifiers.clone(),
        };

        debug!("persisting instance id {}", new_data.instance_id);
        self.storer.set(agent_id, &new_data)?;

        Ok(new_data.instance_id)
    }
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Clone)]
pub struct DataStored {
    pub instance_id: InstanceID,
    pub identifiers: Identifiers,
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::opamp::instance_id::getter::{DataStored, InstanceIDWithIdentifiersGetter};
    use crate::opamp::instance_id::storer::test::MockInstanceIDStorerMock;
    use crate::opamp::instance_id::StorerError;
    use mockall::{mock, predicate};
    use opamp_client::operation::settings::StartSettings;

    mock! {
        pub InstanceIDGetterMock {}

        impl InstanceIDGetter for InstanceIDGetterMock {
            fn get(&self, agent_id: &AgentID) -> Result<InstanceID, GetterError>;
        }
    }

    impl MockInstanceIDGetterMock {
        pub fn should_get(&mut self, agent_id: &AgentID, instance_id: InstanceID) {
            self.expect_get()
                .once()
                .with(predicate::eq(agent_id.clone()))
                .return_once(move |_| Ok(instance_id));
        }
    }

    const AGENT_NAME: &str = "agent1";

    #[test]
    fn test_not_found() {
        let mut mock = MockInstanceIDStorerMock::new();

        let agent_id = AgentID::new(AGENT_NAME).unwrap();
        mock.expect_get()
            .once()
            .with(predicate::eq(agent_id.clone()))
            .returning(|_| Ok(None));
        mock.expect_set()
            .once()
            .with(predicate::eq(agent_id.clone()), predicate::always())
            .returning(|_, _| Ok(()));
        let getter = InstanceIDWithIdentifiersGetter::new(mock, Identifiers::default());
        let res = getter.get(&AgentID::new(AGENT_NAME).unwrap());

        assert!(res.is_ok());
    }

    #[test]
    fn test_error_get() {
        let mut mock = MockInstanceIDStorerMock::new();

        let agent_id = AgentID::new(AGENT_NAME).unwrap();
        mock.expect_get()
            .once()
            .with(predicate::eq(agent_id.clone()))
            .returning(|_| Err(StorerError::Generic));
        let getter = InstanceIDWithIdentifiersGetter::new(mock, Identifiers::default());
        let res = getter.get(&AgentID::new(AGENT_NAME).unwrap());

        assert!(res.is_err());
    }

    #[test]
    fn test_error_set() {
        let mut mock = MockInstanceIDStorerMock::new();

        let agent_id = AgentID::new(AGENT_NAME).unwrap();
        mock.expect_get()
            .once()
            .with(predicate::eq(agent_id.clone()))
            .returning(|_| Ok(None));
        mock.expect_set()
            .once()
            .with(predicate::eq(agent_id.clone()), predicate::always())
            .returning(|_, _| Err(StorerError::Generic));

        let getter = InstanceIDWithIdentifiersGetter::new(mock, Identifiers::default());
        let res = getter.get(&AgentID::new(AGENT_NAME).unwrap());

        assert!(res.is_err());
    }

    #[test]
    fn test_instance_id_already_present() {
        let mut mock = MockInstanceIDStorerMock::new();
        let instance_id = InstanceID::new();
        let agent_id = AgentID::new(AGENT_NAME).unwrap();

        let instance_id_clone = instance_id.clone();
        mock.expect_get()
            .once()
            .with(predicate::eq(agent_id.clone()))
            .return_once(move |_| {
                Ok(Some(DataStored {
                    instance_id: instance_id_clone,
                    identifiers: Default::default(),
                }))
            });
        let getter = InstanceIDWithIdentifiersGetter::new(mock, Identifiers::default());
        let res = getter.get(&AgentID::new(AGENT_NAME).unwrap());

        assert!(res.is_ok());
        assert_eq!(instance_id, res.unwrap());
    }

    #[test]
    fn test_instance_id_present_but_different_identifiers() {
        let mut mock = MockInstanceIDStorerMock::new();
        let instance_id = InstanceID::new();
        let agent_id = AgentID::new(AGENT_NAME).unwrap();

        let instance_id_clone = instance_id.clone();
        mock.expect_get()
            .once()
            .with(predicate::eq(agent_id.clone()))
            .return_once(move |_| {
                Ok(Some(DataStored {
                    instance_id: instance_id_clone,
                    identifiers: get_different_identifier(),
                }))
            });
        mock.expect_set()
            .once()
            .with(predicate::eq(agent_id.clone()), predicate::always())
            .returning(|_, _| Ok(()));
        let getter = InstanceIDWithIdentifiersGetter::new(mock, Identifiers::default());
        let res = getter.get(&AgentID::new(AGENT_NAME).unwrap());

        assert!(res.is_ok());
        assert_ne!(instance_id, res.unwrap());
    }

    #[test]
    fn test_uuid() {
        let uuid_as_str = "018ff38d-01b3-7796-b2c8-1c8069bc6adf";
        // Crete InstanceID from string
        let id = InstanceID::try_from(uuid_as_str).unwrap();
        // Convert instanceID to OpAMP Proto bytes
        let start_settings = StartSettings {
            instance_id: id.clone().into(),
            ..Default::default()
        };
        let id_from_bytes: InstanceID = start_settings.instance_id.clone().try_into().unwrap();

        assert_eq!(id, id_from_bytes);
        assert_eq!(uuid_as_str, format!("{}", id_from_bytes));
    }

    fn get_different_identifier() -> Identifiers {
        #[cfg(feature = "k8s")]
        return Identifiers {
            cluster_name: "test".to_string(),
            fleet_id: "test".to_string(),
        };

        #[cfg(feature = "onhost")]
        return Identifiers {
            machine_id: "different".to_string(),
            hostname: "different".to_string(),
            cloud_instance_id: "different".to_string(),
            ..Default::default()
        };
    }
}
