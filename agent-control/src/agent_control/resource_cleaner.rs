//! Cleanup of resources owned by sub-agents (Kubernetes objects, on-host packages and config).

pub mod k8s_garbage_collector;
pub mod on_host;

use thiserror::Error;

use crate::agent_type::agent_type_id::AgentTypeID;

use super::agent_id::AgentID;
use super::config::SubAgentsMap;

/// Represents a mechanism to clean up resources when called. Intended to be used by Agent Control
/// for cleaning up sub-agent resources, Kubernetes objects or on-host packages.
pub trait ResourceCleaner {
    /// Cleans up resources associated with the given agent ID and agent type ID.
    /// `active_agents` is the full set of agents that remain active after this removal; it is
    /// used to protect shared resources that other agents still declare.
    fn on_agent_removed(
        &self,
        agent_id: &AgentID,
        agent_type: &AgentTypeID,
        active_agents: &SubAgentsMap,
    ) -> Result<(), ResourceCleanerError>;

    /// Cleans up resources that belonged to the old type but are no longer needed by the new type.
    /// `active_agents` is the full set of agents that will be active after this reconciliation
    /// cycle; it is used to protect shared resources that other agents still declare.
    fn on_agent_type_changed(
        &self,
        agent_id: &AgentID,
        old_agent_type: &AgentTypeID,
        new_agent_type: &AgentTypeID,
        active_agents: &SubAgentsMap,
    ) -> Result<(), ResourceCleanerError>;
}

/// Represents an error that occurred during resource cleaning.
#[derive(Debug, Error)]
#[error("resource cleaner error: {0}")]
pub struct ResourceCleanerError(String);

#[cfg(test)]
pub(crate) mod tests {
    use mockall::mock;

    use super::*;

    mock! {
        pub ResourceCleaner {}

        impl ResourceCleaner for ResourceCleaner {
            fn on_agent_removed(
                &self,
                agent_id: &AgentID,
                agent_type: &AgentTypeID,
                active_agents: &SubAgentsMap,
            ) -> Result<(), ResourceCleanerError>;

            fn on_agent_type_changed(
                &self,
                agent_id: &AgentID,
                old_agent_type: &AgentTypeID,
                new_agent_type: &AgentTypeID,
                active_agents: &SubAgentsMap,
            ) -> Result<(), ResourceCleanerError>;
        }
    }

    impl ResourceCleanerError {
        /// Creates a [`ResourceCleanerError`] from the given message.
        pub fn new(msg: &str) -> Self {
            ResourceCleanerError(msg.to_string())
        }
    }
}
