use crate::opamp::instance_id::storer::{InstanceIDStorer, StorerError};
use crate::opamp::instance_id::Identifiers;
use serde::{Deserialize, Serialize};
use tracing::debug;
use ulid::Ulid;

#[derive(thiserror::Error, Debug)]
pub enum GetterError {
    #[error("failed to persist Data: `{0}`")]
    Persisting(#[from] StorerError),
}

// InstanceIDGetter is a shared trait implemented differently onHost and onK8s.
// The two implementations are behind a config feature flag.
pub trait InstanceIDGetter {
    fn get(&self, agent_fqdn: &str, identifiers: &Identifiers) -> Result<String, GetterError>;
}

pub struct ULIDInstanceIDGetter<T>
where
    T: InstanceIDStorer,
{
    storer: T,
}

impl<T> ULIDInstanceIDGetter<T>
where
    T: InstanceIDStorer,
{
    pub fn new(storer: T) -> Self {
        Self { storer }
    }
}

impl<T> InstanceIDGetter for ULIDInstanceIDGetter<T>
where
    T: InstanceIDStorer,
{
    fn get(&self, agent_fqdn: &str, identifiers: &Identifiers) -> Result<String, GetterError> {
        debug!("retrieving ulid");
        let data = self.storer.get(agent_fqdn)?;

        match data {
            None => {
                debug!("storer returned no data");
            }
            Some(d) => {
                if d.identifiers == *identifiers {
                    return Ok(d.ulid.to_string());
                }
                debug!(
                    "stored data had different identifiers {:?}!={:?}",
                    d.identifiers, *identifiers
                );
            }
        }

        let new_data = DataStored {
            ulid: Ulid::new(),
            identifiers: identifiers.clone(),
        };

        debug!("persisting ulid {}", new_data.ulid);
        self.storer.set(agent_fqdn, &new_data)?;

        Ok(new_data.ulid.to_string())
    }
}

#[derive(Deserialize, Serialize)]
pub struct DataStored {
    pub ulid: Ulid,
    pub identifiers: Identifiers,
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::opamp::instance_id::getter::{DataStored, InstanceIDGetter, ULIDInstanceIDGetter};
    use crate::opamp::instance_id::storer::test::MockInstanceIDStorerMock;
    use crate::opamp::instance_id::storer::StorerError;
    use mockall::{mock, predicate};

    mock! {
        pub InstanceIDGetterMock {}

        impl InstanceIDGetter for InstanceIDGetterMock {
            fn get(&self, agent_fqdn: &str, identifiers: &Identifiers) -> Result<String, GetterError>;
        }
    }

    impl MockInstanceIDGetterMock {
        pub fn should_get(&mut self, agent_fqdn: String, ulid: String) {
            self.expect_get()
                .once()
                .with(
                    predicate::eq(agent_fqdn.clone()),
                    predicate::eq(Identifiers::default()),
                )
                .returning(move |_, _| Ok(ulid.clone()));
        }
    }

    #[test]
    fn test_not_found() {
        let mut mock = MockInstanceIDStorerMock::new();

        mock.expect_get().once().returning(|_| Ok(None));
        mock.expect_set().once().returning(|_, _| Ok(()));
        let getter = ULIDInstanceIDGetter::new(mock);
        let res = getter.get("agent_fqdn", &Identifiers::default());

        assert!(res.is_ok());
    }

    #[test]
    fn test_error_get() {
        let mut mock = MockInstanceIDStorerMock::new();

        mock.expect_get()
            .once()
            .returning(|_| Err(StorerError::Generic));
        let getter = ULIDInstanceIDGetter::new(mock);
        let res = getter.get("agent_fqdn", &Identifiers::default());

        assert!(res.is_err());
    }

    #[test]
    fn test_error_set() {
        let mut mock = MockInstanceIDStorerMock::new();

        mock.expect_get().once().returning(|_| Ok(None));
        mock.expect_set()
            .once()
            .returning(|_, _| Err(StorerError::Generic));

        let getter = ULIDInstanceIDGetter::new(mock);
        let res = getter.get("agent_fqdn", &Identifiers::default());

        assert!(res.is_err());
    }

    #[test]
    fn test_ulid_already_present() {
        let mut mock = MockInstanceIDStorerMock::new();
        let ulid = ulid::Ulid::new();

        mock.expect_get().once().returning(move |_| {
            Ok(Some(DataStored {
                ulid,
                identifiers: Default::default(),
            }))
        });
        let getter = ULIDInstanceIDGetter::new(mock);
        let res = getter.get("agent_fqdn", &Identifiers::default());

        assert!(res.is_ok());
        assert_eq!(ulid.to_string(), res.unwrap());
    }

    #[test]
    fn test_ulid_present_but_different_identifiers() {
        let agent_fqdn = "agent.example.com";
        let mut mock = MockInstanceIDStorerMock::new();
        let ulid = ulid::Ulid::new();

        mock.expect_get().once().returning(move |_| {
            Ok(Some(DataStored {
                ulid,
                identifiers: get_different_identifier(),
            }))
        });
        mock.expect_set().once().returning(|_, _| Ok(()));
        let getter = ULIDInstanceIDGetter::new(mock);
        let res = getter.get(agent_fqdn, &Identifiers::default());

        assert!(res.is_ok());
        assert_ne!(ulid.to_string(), res.unwrap());
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
