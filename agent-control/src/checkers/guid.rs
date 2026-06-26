//! Checking and reporting the entity GUID of a managed agent.
use std::fmt::{Debug, Display, Formatter};
/// Kubernetes implementation of the GUID checker.
pub mod k8s;

/// An entity GUID retrieved from an agent, together with the OpAMP field it maps to.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityGuid {
    /// The retrieved entity GUID.
    pub guid: String,
    /// The OpAMP attribute field where the GUID will be reported.
    pub opamp_field: String,
}

/// Error returned when a GUID check fails.
#[derive(thiserror::Error, Debug)]
pub struct GuidCheckError(
    /// The error message.
    pub String,
);

impl Display for GuidCheckError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A type that can retrieve the entity GUID of a managed agent.
pub trait GuidChecker {
    /// Retrieves the agent's entity GUID.
    fn check_guid(&self) -> Result<EntityGuid, GuidCheckError>;
}
