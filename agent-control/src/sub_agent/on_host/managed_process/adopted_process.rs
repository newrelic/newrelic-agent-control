//! A running process Agent Control did not spawn.

use crate::agent_control::agent_id::AgentID;
use std::time::Duration;

/// A running process Agent Control did not spawn.
pub struct AdoptedProcess {
    agent_id: AgentID,
    pid: u32,
    shutdown_timeout: Duration,
}

impl AdoptedProcess {
    pub(super) fn agent_id(&self) -> &AgentID {
        &self.agent_id
    }

    pub(super) fn get_pid(&self) -> u32 {
        self.pid
    }

    pub(super) fn shutdown_timeout(&self) -> Duration {
        self.shutdown_timeout
    }
}
