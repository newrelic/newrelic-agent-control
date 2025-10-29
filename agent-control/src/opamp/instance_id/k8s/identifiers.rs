use crate::opamp::instance_id::definition::InstanceIdentifiers;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

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
