use crate::opamp::instance_id::getter::{IdentifiersRetriever, ULIDInstanceIDGetter};
use crate::opamp::instance_id::{Storer, StorerError};
use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Deserialize, Serialize, PartialEq, Clone)]
pub struct Identifiers {
    pub hostname: String,
    pub machine_id: String,
}

#[derive(thiserror::Error, Debug)]
pub enum GetterError {
    #[error("failed to persist Data: `{0}`")]
    Persisting(#[from] StorerError),
}

pub struct OnHostIdentifiersRetriever {}

impl IdentifiersRetriever for OnHostIdentifiersRetriever {
    fn get() -> Result<Identifiers, GetterError> {
        Ok(Identifiers::default())
    }
}

impl ULIDInstanceIDGetter<Storer> {
    pub fn try_with_identifiers<I>() -> Result<Self, GetterError>
    where
        I: IdentifiersRetriever,
    {
        Ok(Self::new(Storer::new(), I::get()?))
    }
}
