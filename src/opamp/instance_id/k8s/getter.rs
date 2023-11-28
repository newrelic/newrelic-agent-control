use crate::k8s;
use crate::opamp::instance_id::getter::ULIDInstanceIDGetter;
use crate::opamp::instance_id::k8s::storer::{Storer, StorerError};
use serde::{Deserialize, Serialize};

#[cfg_attr(test, mockall_double::double)]
use crate::k8s::executor::K8sExecutor;

#[derive(Default, Debug, Deserialize, Serialize, PartialEq, Clone)]
pub struct Identifiers {
    pub cluster_name: String,
}

pub fn get_identifiers(cluster_name: String) -> Identifiers {
    Identifiers { cluster_name }
}

#[derive(thiserror::Error, Debug)]
pub enum GetterError {
    #[error("failed to persist Data: `{0}`")]
    Persisting(#[from] StorerError),

    #[error("Initialising client: `{0}`")]
    K8sClientInitialization(#[from] k8s::Error),
}

impl ULIDInstanceIDGetter<Storer> {
    pub async fn try_with_identifiers(
        namespace: String,
        identifiers: Identifiers,
    ) -> Result<Self, GetterError> {
        Ok(Self::new(
            Storer::new(K8sExecutor::try_default(namespace).await?),
            identifiers,
        ))
    }
}
