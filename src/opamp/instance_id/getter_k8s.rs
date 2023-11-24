use crate::k8s;
use crate::k8s::executor::K8sExecutor;
use crate::opamp::instance_id::getter::{IdentifiersRetriever, ULIDInstanceIDGetter};
use crate::opamp::instance_id::{Storer, StorerError};
use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Deserialize, Serialize, PartialEq, Clone)]
pub struct Identifiers {
    pub cluster_name: String,
}

#[derive(thiserror::Error, Debug)]
pub enum GetterError {
    #[error("failed to persist Data: `{0}`")]
    Persisting(#[from] StorerError),

    #[error("k8s api failure: `{0}`")]
    K8s(#[from] k8s::Error),
}

const CM_PREFIX: &str = "super-agent-ulid";

pub struct K8sIdentifiersRetriever {}

impl IdentifiersRetriever for K8sIdentifiersRetriever {
    fn get() -> Result<Identifiers, GetterError> {
        Ok(Identifiers::default())
    }
}

impl ULIDInstanceIDGetter<Storer> {
    pub async fn try_with_identifiers<I>(namespace: String) -> Result<Self, GetterError>
    where
        I: IdentifiersRetriever,
    {
        Ok(Self::new(
            Storer::new(K8sExecutor::try_default(namespace).await?),
            I::get()?,
        ))
    }
}
