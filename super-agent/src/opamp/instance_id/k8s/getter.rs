use crate::k8s;
use crate::k8s::store::K8sStore;
use crate::opamp::instance_id::getter::ULIDInstanceIDGetter;
use crate::opamp::instance_id::k8s::storer::{Storer, StorerError};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::sync::Arc;

#[derive(Default, Debug, Deserialize, Serialize, PartialEq, Clone)]
pub struct Identifiers {
    pub cluster_name: String,
}

impl Display for Identifiers {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "cluster_name = '{}'", self.cluster_name)
    }
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
    pub fn try_with_identifiers(
        k8s_store: Arc<K8sStore>,
        identifiers: Identifiers,
    ) -> Result<Self, GetterError> {
        Ok(Self::new(Storer::new(k8s_store), identifiers))
    }
}
