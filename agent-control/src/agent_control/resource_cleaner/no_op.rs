use crate::{agent_control::agent_id::AgentID, agent_type::agent_type_id::AgentTypeID};

use super::{ResourceCleaner, ResourceCleanerError};

/// Basic implementation of a no-op ResourceCleaner.
pub struct NoOpResourceCleaner;

impl ResourceCleaner for NoOpResourceCleaner {
    fn clean(&self, _: &AgentID, _: &AgentTypeID) -> Result<(), ResourceCleanerError> {
        Ok(())
    }
}
