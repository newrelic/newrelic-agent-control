use crate::k8s::store::K8sStore;
use crate::opamp::instance_id::definition::InstanceIdentifiers;
use crate::opamp::instance_id::getter::InstanceIDWithIdentifiersGetter;
use crate::opamp::instance_id::k8s::storer::Storer;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::sync::Arc;

#[derive(Default, Debug, Deserialize, Serialize, PartialEq, Clone)]
pub struct Identifiers {
    pub cluster_name: String,
    pub fleet_id: String,
}

impl InstanceIdentifiers for Identifiers {}

impl Display for Identifiers {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "cluster_name = '{}', fleet_id = '{}'",
            self.cluster_name, self.fleet_id
        )
    }
}

pub fn get_identifiers(cluster_name: String, fleet_id: String) -> Identifiers {
    Identifiers {
        cluster_name,
        fleet_id,
    }
}

impl InstanceIDWithIdentifiersGetter<Storer> {
    pub fn new_k8s_instance_id_getter(k8s_store: Arc<K8sStore>, identifiers: Identifiers) -> Self {
        Self::new(Storer::new(k8s_store), identifiers)
    }
}
