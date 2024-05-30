use super::{GetterError, Identifiers};
use crate::{opamp::instance_id::storer::InstanceIDStorer, super_agent::config::AgentID};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use tracing::debug;
use uuid::Uuid;

// InstanceID holds the to_string of Uuid assigned to a Agent
#[derive(Default, Debug, Deserialize, Serialize, PartialEq, Clone, Eq, Hash)]
pub struct InstanceID(Uuid);

impl InstanceID {
    pub fn new(uuid: Uuid) -> InstanceID {
        InstanceID(uuid)
    }

    pub fn get_uuid(&self) -> Uuid {
        self.0
    }
}

impl Display for InstanceID {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// InstanceIDGetter returns an InstanceID for a specific agentID.
pub trait InstanceIDGetter {
    fn get(&self, agent_id: &AgentID) -> Result<InstanceID, GetterError>;
}

pub struct UUIDInstanceIDGetter<S>
where
    S: InstanceIDStorer,
{
    storer: S,
    identifiers: Identifiers,
}

impl<S> UUIDInstanceIDGetter<S>
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

impl<S> InstanceIDGetter for UUIDInstanceIDGetter<S>
where
    S: InstanceIDStorer,
{
    fn get(&self, agent_id: &AgentID) -> Result<InstanceID, GetterError> {
        debug!("retrieving uuid");
        let data = self.storer.get(agent_id)?;

        match data {
            None => {
                debug!("storer returned no data");
            }
            Some(d) if d.identifiers == self.identifiers => return Ok(d.uuid),
            Some(d) => debug!(
                "stored data had different identifiers {:?}!={:?}",
                d.identifiers, self.identifiers
            ),
        }

        let new_data = DataStored {
            uuid: InstanceID(Uuid::now_v7()),
            identifiers: self.identifiers.clone(),
        };

        debug!("persisting uuid {}", new_data.uuid);
        self.storer.set(agent_id, &new_data)?;

        Ok(new_data.uuid)
    }
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Clone)]
pub struct DataStored {
    pub uuid: InstanceID,
    pub identifiers: Identifiers,
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::opamp::instance_id::getter::{DataStored, UUIDInstanceIDGetter};
    use crate::opamp::instance_id::storer::test::MockInstanceIDStorerMock;
    use crate::opamp::instance_id::StorerError;
    use mockall::{mock, predicate};

    mock! {
        pub InstanceIDGetterMock {}

        impl InstanceIDGetter for InstanceIDGetterMock {
            fn get(&self, agent_id: &AgentID) -> Result<InstanceID, GetterError>;
        }
    }

    impl MockInstanceIDGetterMock {
        pub fn should_get(&mut self, agent_id: &AgentID, uuid: Uuid) {
            self.expect_get()
                .once()
                .with(predicate::eq(agent_id.clone()))
                .returning(move |_| Ok(InstanceID(uuid)));
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
        let getter = UUIDInstanceIDGetter::new(mock, Identifiers::default());
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
        let getter = UUIDInstanceIDGetter::new(mock, Identifiers::default());
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

        let getter = UUIDInstanceIDGetter::new(mock, Identifiers::default());
        let res = getter.get(&AgentID::new(AGENT_NAME).unwrap());

        assert!(res.is_err());
    }

    #[test]
    fn test_uuid_already_present() {
        let mut mock = MockInstanceIDStorerMock::new();
        let uuid = Uuid::now_v7();
        let agent_id = AgentID::new(AGENT_NAME).unwrap();

        mock.expect_get()
            .once()
            .with(predicate::eq(agent_id.clone()))
            .returning(move |_| {
                Ok(Some(DataStored {
                    uuid: InstanceID(uuid),
                    identifiers: Default::default(),
                }))
            });
        let getter = UUIDInstanceIDGetter::new(mock, Identifiers::default());
        let res = getter.get(&AgentID::new(AGENT_NAME).unwrap());

        assert!(res.is_ok());
        assert_eq!(InstanceID(uuid), res.unwrap());
    }

    #[test]
    fn test_uuid_present_but_different_identifiers() {
        let mut mock = MockInstanceIDStorerMock::new();
        let uuid = Uuid::now_v7();
        let agent_id = AgentID::new(AGENT_NAME).unwrap();

        mock.expect_get()
            .once()
            .with(predicate::eq(agent_id.clone()))
            .returning(move |_| {
                Ok(Some(DataStored {
                    uuid: InstanceID(uuid),
                    identifiers: get_different_identifier(),
                }))
            });
        mock.expect_set()
            .once()
            .with(predicate::eq(agent_id.clone()), predicate::always())
            .returning(|_, _| Ok(()));
        let getter = UUIDInstanceIDGetter::new(mock, Identifiers::default());
        let res = getter.get(&AgentID::new(AGENT_NAME).unwrap());

        assert!(res.is_ok());
        assert_ne!(InstanceID(uuid), res.unwrap());
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
