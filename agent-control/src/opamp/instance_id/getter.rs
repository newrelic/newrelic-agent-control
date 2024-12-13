use super::{GetterError, Identifiers, InstanceID};
use crate::{agent_control::config::AgentID, opamp::instance_id::storer::InstanceIDStorer};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tracing::debug;

// IDGetter returns an InstanceID for a specific agentID.
pub trait InstanceIDGetter {
    fn get(&self, agent_id: &AgentID) -> Result<InstanceID, GetterError>;
}

pub struct InstanceIDWithIdentifiersGetter<S>
where
    S: InstanceIDStorer,
{
    storer: Mutex<S>,
    identifiers: Identifiers,
}

impl<S> InstanceIDWithIdentifiersGetter<S>
where
    S: InstanceIDStorer,
{
    pub fn new(storer: S, identifiers: Identifiers) -> Self {
        Self {
            storer: Mutex::new(storer),
            identifiers,
        }
    }

    pub fn with_identifiers(self, identifiers: Identifiers) -> Self {
        Self {
            identifiers,
            ..self
        }
    }
}

impl<S> InstanceIDGetter for InstanceIDWithIdentifiersGetter<S>
where
    S: InstanceIDStorer,
{
    fn get(&self, agent_id: &AgentID) -> Result<InstanceID, GetterError> {
        let storer = self.storer.lock().expect("failed to acquire the lock");
        debug!(%agent_id, "retrieving instance id");
        let data = storer.get(agent_id)?;

        match data {
            None => {
                debug!(%agent_id, "storer returned no data");
            }
            Some(d) if d.identifiers == self.identifiers => return Ok(d.instance_id),
            Some(d) => debug!(
                %agent_id,
                "stored data had different identifiers {:?}!={:?}",
                d.identifiers, self.identifiers
            ),
        }

        let new_data = DataStored {
            instance_id: InstanceID::create(),
            identifiers: self.identifiers.clone(),
        };

        debug!(%agent_id, "persisting instance id {}", new_data.instance_id);
        storer.set(agent_id, &new_data)?;

        Ok(new_data.instance_id)
    }
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Clone)]
pub struct DataStored {
    pub instance_id: InstanceID,
    pub identifiers: Identifiers,
}

#[cfg(test)]
pub mod tests {
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use super::*;
    use crate::opamp::instance_id::getter::{DataStored, InstanceIDWithIdentifiersGetter};
    use crate::opamp::instance_id::storer::tests::MockInstanceIDStorerMock;
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
        let getter = InstanceIDWithIdentifiersGetter::new(mock, Identifiers::default());
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
        let getter = InstanceIDWithIdentifiersGetter::new(mock, Identifiers::default());
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

        let getter = InstanceIDWithIdentifiersGetter::new(mock, Identifiers::default());
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
