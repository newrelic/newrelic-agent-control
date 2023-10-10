use std::fmt::{Display, Formatter};

use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq, Clone, Default)]
pub struct AgentMetadata {
    pub name: String,
    pub namespace: String,
    pub version: String,
}

impl Display for AgentMetadata {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}:{}", self.namespace, self.name, self.version)
    }
}
