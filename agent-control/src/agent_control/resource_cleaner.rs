pub mod k8s_garbage_collector;
pub(super) mod no_op;

use thiserror::Error;

use crate::agent_type::agent_type_id::AgentTypeID;

use super::agent_id::AgentID;

/// Represents a mechanism to clean up resources when called. Intended to be used by Agent Control
/// for cleaning up sub-agent resources, Kubernetes objects or on-host packages.
pub trait ResourceCleaner {
    /// Cleans up resources associated with the given agent ID and agent type ID.
    fn clean(&self, agent_id: &AgentID, config: &AgentTypeID) -> Result<(), ResourceCleanerError>;
}

/// Represents an error that occurred during resource cleaning.
#[derive(Debug, Error)]
#[error("Resource cleaner error: {0}")]
pub struct ResourceCleanerError(String);
