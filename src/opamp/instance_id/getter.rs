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
        let _ = self
            .storer
            .set(agent_fqdn, &new_data)
            .map_err(|err| debug!("error while persisting ULID={}: {}", new_data.ulid, err));

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
}
