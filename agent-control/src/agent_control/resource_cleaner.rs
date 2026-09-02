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
    /// Cleans up all resources associated with the given agent ID after it has been removed from
    /// the fleet. `active_agents` is the full set of agents that remain active after this removal.
    fn on_agent_removed(
        &self,
        agent_id: &AgentID,
        agent_type: &AgentTypeID,
        active_agents: &SubAgentsMap,
    ) -> Result<(), ResourceCleanerError>;

    /// Cleans up stale resources after a version bump (same type name, different version).
    /// The agent's identity (OpAMP instance ID) is preserved. `active_agents` is the full set of
    /// agents active after this reconciliation cycle.
    fn on_agent_version_bumped(
        &self,
        agent_id: &AgentID,
        old_agent_type: &AgentTypeID,
        new_agent_type: &AgentTypeID,
        active_agents: &SubAgentsMap,
    ) -> Result<(), ResourceCleanerError>;

    /// Cleans up all per-agent resources after the agent type is replaced with a different one
    /// (namespace or name changed). The agent starts fresh with a new identity on its next
    /// connection. `active_agents` is the full set of agents active after this reconciliation
    /// cycle.
    fn on_agent_type_replaced(
        &self,
        agent_id: &AgentID,
        old_agent_type: &AgentTypeID,
        new_agent_type: &AgentTypeID,
        active_agents: &SubAgentsMap,
    ) -> Result<(), ResourceCleanerError>;

    /// Dispatches to [`on_agent_version_bumped`](Self::on_agent_version_bumped) or
    /// [`on_agent_type_replaced`](Self::on_agent_type_replaced) based on whether the type name
    /// changed. Implementers override the two specific methods, not this one.
    fn on_agent_type_changed(
        &self,
        agent_id: &AgentID,
        old_agent_type: &AgentTypeID,
        new_agent_type: &AgentTypeID,
        active_agents: &SubAgentsMap,
    ) -> Result<(), ResourceCleanerError> {
        if old_agent_type.is_same_type(new_agent_type) {
            self.on_agent_version_bumped(agent_id, old_agent_type, new_agent_type, active_agents)
        } else {
            self.on_agent_type_replaced(agent_id, old_agent_type, new_agent_type, active_agents)
        }
    }
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

            fn on_agent_version_bumped(
                &self,
                agent_id: &AgentID,
                old_agent_type: &AgentTypeID,
                new_agent_type: &AgentTypeID,
                active_agents: &SubAgentsMap,
            ) -> Result<(), ResourceCleanerError>;

            fn on_agent_type_replaced(
                &self,
                agent_id: &AgentID,
                old_agent_type: &AgentTypeID,
                new_agent_type: &AgentTypeID,
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
