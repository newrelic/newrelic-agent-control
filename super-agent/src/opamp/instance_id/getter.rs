use super::{GetterError, Identifiers};
use crate::config::super_agent_configs::AgentID;
use crate::opamp::instance_id::storer::InstanceIDStorer;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use tracing::debug;
use ulid::Ulid;

// InstanceID holds the to_string of Ulid assigned to a Agent
#[derive(Default, Debug, Deserialize, Serialize, PartialEq, Clone)]
pub struct InstanceID(String);

impl InstanceID {
    pub(crate) fn new(id: String) -> InstanceID {
        InstanceID(id)
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

pub struct ULIDInstanceIDGetter<S>
where
    S: InstanceIDStorer,
{
    storer: S,
    identifiers: Identifiers,
}

impl<S> ULIDInstanceIDGetter<S>
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

impl<S> InstanceIDGetter for ULIDInstanceIDGetter<S>
where
    S: InstanceIDStorer,
{
    fn get(&self, agent_id: &AgentID) -> Result<InstanceID, GetterError> {
        debug!("retrieving ulid");
        let data = self.storer.get(agent_id)?;

        match data {
            None => {
                debug!("storer returned no data");
            }
            Some(d) if d.identifiers == self.identifiers => return Ok(d.ulid),
            Some(d) => debug!(
                "stored data had different identifiers {:?}!={:?}",
                d.identifiers, self.identifiers
            ),
        }

        let new_data = DataStored {
            ulid: InstanceID(Ulid::new().to_string()),
            identifiers: self.identifiers.clone(),
        };

        debug!("persisting ulid {}", new_data.ulid);
        self.storer.set(agent_id, &new_data)?;

        Ok(new_data.ulid)
    }
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Clone)]
pub struct DataStored {
    pub ulid: InstanceID,
    pub identifiers: Identifiers,
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::opamp::instance_id::getter::{DataStored, ULIDInstanceIDGetter};
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
        pub fn should_get(&mut self, agent_id: &AgentID, ulid: String) {
            self.expect_get()
                .once()
                .with(predicate::eq(agent_id.clone()))
                .returning(move |_| Ok(InstanceID(ulid.clone())));
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
        let getter = ULIDInstanceIDGetter::new(mock, Identifiers::default());
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
        let getter = ULIDInstanceIDGetter::new(mock, Identifiers::default());
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

        let getter = ULIDInstanceIDGetter::new(mock, Identifiers::default());
        let res = getter.get(&AgentID::new(AGENT_NAME).unwrap());

        assert!(res.is_err());
    }

    #[test]
    fn test_ulid_already_present() {
        let mut mock = MockInstanceIDStorerMock::new();
        let ulid = Ulid::new();
        let agent_id = AgentID::new(AGENT_NAME).unwrap();

        mock.expect_get()
            .once()
            .with(predicate::eq(agent_id.clone()))
            .returning(move |_| {
                Ok(Some(DataStored {
                    ulid: InstanceID(ulid.to_string()),
                    identifiers: Default::default(),
                }))
            });
        let getter = ULIDInstanceIDGetter::new(mock, Identifiers::default());
        let res = getter.get(&AgentID::new(AGENT_NAME).unwrap());

        assert!(res.is_ok());
        assert_eq!(InstanceID(ulid.to_string()), res.unwrap());
    }

    #[test]
    fn test_ulid_present_but_different_identifiers() {
        let mut mock = MockInstanceIDStorerMock::new();
        let ulid = ulid::Ulid::new();
        let agent_id = AgentID::new(AGENT_NAME).unwrap();

        mock.expect_get()
            .once()
            .with(predicate::eq(agent_id.clone()))
            .returning(move |_| {
                Ok(Some(DataStored {
                    ulid: InstanceID(ulid.to_string()),
                    identifiers: get_different_identifier(),
                }))
            });
        mock.expect_set()
            .once()
            .with(predicate::eq(agent_id.clone()), predicate::always())
            .returning(|_, _| Ok(()));
        let getter = ULIDInstanceIDGetter::new(mock, Identifiers::default());
        let res = getter.get(&AgentID::new(AGENT_NAME).unwrap());

        assert!(res.is_ok());
        assert_ne!(InstanceID(ulid.to_string()), res.unwrap());
    }

    fn get_different_identifier() -> Identifiers {
        #[cfg(all(not(feature = "onhost"), feature = "k8s"))]
        return Identifiers {
            cluster_name: "test".to_string(),
        };

        #[cfg(feature = "onhost")]
        return Identifiers {
            machine_id: "different".to_string(),
            hostname: "different".to_string(),
        };
    }
}
