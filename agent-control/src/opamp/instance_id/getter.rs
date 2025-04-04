use crate::k8s;
use crate::{agent_control::agent_id::AgentID, opamp::instance_id::storer::InstanceIDStorer};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tracing::debug;

use super::{definition::InstanceIdentifiers, InstanceID};

// IDGetter returns an InstanceID for a specific agentID.
pub trait InstanceIDGetter {
    fn get(&self, agent_id: &AgentID) -> Result<InstanceID, GetterError>;
}

#[derive(thiserror::Error, Debug)]
pub enum GetterError {
    #[error("failed to persist data: `{0}`")]
    OnHostPersisting(#[from] super::on_host::storer::StorerError),

    #[error("failed to persist k8s data: `{0}`")]
    K8sPersisting(#[from] super::k8s::storer::StorerError),

    #[error("Initialising client: `{0}`")]
    K8sClientInitialization(#[from] k8s::Error),

    #[cfg(test)]
    #[error("mock getter error")]
    MockGetterError,
}

pub struct InstanceIDWithIdentifiersGetter<S>
where
    S: InstanceIDStorer,
    GetterError: From<S::Error>,
{
    storer: Mutex<S>,
    identifiers: S::Identifiers,
}

impl<S> InstanceIDWithIdentifiersGetter<S>
where
    S: InstanceIDStorer,
    GetterError: From<S::Error>,
{
    pub fn new(storer: S, identifiers: S::Identifiers) -> Self {
        Self {
            storer: Mutex::new(storer),
            identifiers,
        }
    }

    pub fn with_identifiers(self, identifiers: S::Identifiers) -> Self {
        Self {
            identifiers,
            ..self
        }
    }
}

impl<S> InstanceIDGetter for InstanceIDWithIdentifiersGetter<S>
where
    S: InstanceIDStorer,
    GetterError: From<S::Error>,
{
    fn get(&self, agent_id: &AgentID) -> Result<InstanceID, GetterError> {
        let storer = self.storer.lock().expect("failed to acquire the lock");
        debug!("retrieving instance id");
        let data = storer.get(agent_id)?;

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
            instance_id: InstanceID::create(),
            identifiers: self.identifiers.clone(),
        };

        debug!("persisting instance id {}", new_data.instance_id);
        storer.set(agent_id, &new_data)?;

        Ok(new_data.instance_id)
    }
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Clone)]
pub struct DataStored<I: InstanceIdentifiers> {
    pub instance_id: InstanceID,
    pub identifiers: I,
}

#[cfg(test)]
pub mod tests {
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use super::*;
    use crate::opamp::instance_id::definition::tests::MockIdentifiers;
    use crate::opamp::instance_id::getter::{DataStored, InstanceIDWithIdentifiersGetter};
    use crate::opamp::instance_id::storer::tests::{MockInstanceIDStorerMock, MockStorerError};
    use mockall::{mock, predicate};
    use opamp_client::operation::instance_uid::InstanceUid;

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
        let getter = InstanceIDWithIdentifiersGetter::new(mock, MockIdentifiers::default());
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
            .returning(|_| Err(MockStorerError));
        let getter = InstanceIDWithIdentifiersGetter::new(mock, MockIdentifiers::default());
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
            .returning(|_, _| Err(MockStorerError));

        let getter = InstanceIDWithIdentifiersGetter::new(mock, MockIdentifiers::default());
        let res = getter.get(&AgentID::new(AGENT_NAME).unwrap());

        assert!(res.is_err());
    }

    #[test]
    fn test_instance_id_already_present() {
        let mut mock = MockInstanceIDStorerMock::new();
        let instance_id = InstanceID::create();
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
        let getter = InstanceIDWithIdentifiersGetter::new(mock, MockIdentifiers::default());
        let res = getter.get(&AgentID::new(AGENT_NAME).unwrap());

        assert!(res.is_ok());
        assert_eq!(instance_id, res.unwrap());
    }

    #[test]
    fn test_instance_id_present_but_different_identifiers() {
        let mut mock = MockInstanceIDStorerMock::new();
        let instance_id = InstanceID::create();
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
        let getter = InstanceIDWithIdentifiersGetter::new(mock, MockIdentifiers::default());
        let res = getter.get(&AgentID::new(AGENT_NAME).unwrap());

        assert!(res.is_ok());
        assert_ne!(instance_id, res.unwrap());
    }

    #[test]
    fn test_thread_safety() {
        let mut mock = MockInstanceIDStorerMock::new();

        let agent_id = AgentID::new(AGENT_NAME).unwrap();
        // Data is read twice: first time it returns nothing, second time it returns data
        mock.expect_get()
            .once()
            .with(predicate::eq(agent_id.clone()))
            .returning(|_| Ok(None));
        mock.expect_get()
            .once()
            .with(predicate::eq(agent_id.clone()))
            .return_once(move |_| {
                Ok(Some(DataStored {
                    instance_id: InstanceID::create(),
                    identifiers: Default::default(),
                }))
            });
        // Data is written just once
        mock.expect_set()
            .once()
            .with(predicate::eq(agent_id.clone()), predicate::always())
            .returning(|_, _| {
                thread::sleep(Duration::from_millis(500)); // Make write slow to assure issues if resources are not protected
                Ok(())
            });

        let getter = InstanceIDWithIdentifiersGetter::new(mock, MockIdentifiers::default());
        let getter1 = Arc::new(getter);
        let getter2 = getter1.clone();

        let t1 = thread::spawn(move || {
            let res = getter1.get(&AgentID::new(AGENT_NAME).unwrap());
            assert!(res.is_ok());
        });
        let t2 = thread::spawn(move || {
            let res = getter2.get(&AgentID::new(AGENT_NAME).unwrap());
            assert!(res.is_ok());
        });
        t1.join().unwrap();
        t2.join().unwrap();
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
        assert_eq!(uuid_as_str, format!("{}", id_from_bytes));
    }

    fn get_different_identifier() -> MockIdentifiers {
        MockIdentifiers(1)
    }
}
