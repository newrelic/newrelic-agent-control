use crate::agent_type::agent_type_id::AgentTypeID;

/// Represents ownership of a resource in Agent Control's data store.
///
/// This type is used to distinguish between resources owned by Agent Control itself
/// (internal resources like fleet-data ConfigMaps for AC, instance IDs) and resources
/// owned by sub-agents (fleet-data ConfigMaps for sub-agents, supervisor-created K8s objects).
///
/// The ownership determines which annotations are applied to the resource:
/// - `AgentControl`: applies `owned-by=agent-control` annotation
/// - `SubAgent`: applies `owned-by=sub-agent` + `agent-type-id=<type>` annotations
///
/// This type-safe approach prevents bugs where the wrong ownership could be accidentally
/// applied, which would cause resources to be incorrectly handled by the garbage collector.
#[derive(Debug, Clone, PartialEq)]
pub enum ResourceOwnership {
    /// Resource is owned by Agent Control itself (e.g., AC's own fleet-data ConfigMap, instance IDs)
    AgentControl,
    /// Resource is owned by a sub-agent (e.g., sub-agent's fleet-data ConfigMap, supervisor-created objects)
    SubAgent(AgentTypeID),
}

impl ResourceOwnership {
    /// Returns the agent type ID if this is a sub-agent-owned resource
    pub fn agent_type_id(&self) -> Option<&AgentTypeID> {
        match self {
            ResourceOwnership::AgentControl => None,
            ResourceOwnership::SubAgent(agent_type_id) => Some(agent_type_id),
        }
    }

    /// Returns true if this resource is owned by Agent Control
    pub fn is_agent_control(&self) -> bool {
        matches!(self, ResourceOwnership::AgentControl)
    }

    /// Returns true if this resource is owned by a sub-agent
    pub fn is_sub_agent(&self) -> bool {
        matches!(self, ResourceOwnership::SubAgent(_))
    }
}
