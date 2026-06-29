//! Identifiers used to bind an instance id to a Kubernetes deployment.
use crate::opamp::instance_id::definition::InstanceIdentifiers;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

/// Kubernetes identifiers that bind an instance id to a cluster and fleet.
#[derive(Default, Debug, Deserialize, Serialize, PartialEq, Clone)]
pub struct Identifiers {
    /// Name of the Kubernetes cluster.
    pub cluster_name: String,
    /// Fleet identifier for fleet management.
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

/// Builds [`Identifiers`] from the given cluster name and fleet id.
pub fn get_identifiers(cluster_name: String, fleet_id: String) -> Identifiers {
    Identifiers {
        cluster_name,
        fleet_id,
    }
}
